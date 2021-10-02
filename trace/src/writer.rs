//! Abstractions for creating [`io::Write`] instances.
//!
//! [`io::Write`]: mycelium_util::io::Write
use mycelium_util::{
    fmt::{self, Debug},
    io::{self, Write},
    sync::spin::{Mutex, MutexGuard},
};
use tracing_core::Metadata;

/// A type that can create [`io::Write`] instances.
///
/// This trait is already implemented for function pointers and
/// immutably-borrowing closures that return an instance of [`io::Write`],
/// Additionally, it is implemented for [`mycelium_util::sync::spin::Mutex`]
/// when the type inside the mutex implements [`io::Write`].
///
/// The [`MakeWriter::make_writer_for`] method takes [`Metadata`] describing a
/// span or event and returns a writer. `MakeWriter`s can optionally provide
/// implementations of this method with behaviors that differ based on the span
/// or event being written. For example, events at different [levels] might be
/// written to different output streams, or data from different [targets] might
/// be written to separate log files. When the `MakeWriter` has no custom
/// behavior based on metadata, the default implementation of `make_writer_for`
/// simply calls `self.make_writer()`, ignoring the metadata. Therefore, when
/// metadata _is_ available, callers should prefer to call `make_writer_for`,
/// passing in that metadata, so that the `MakeWriter` implementation can choose
/// the appropriate behavior.
///
/// [`io::Write`]: mycelium_util::io::Write
/// [`Event`]: tracing_core::event::Event
/// [`MakeWriter::make_writer_for`]: MakeWriter::make_writer_for
/// [`Metadata`]: tracing_core::Metadata
/// [levels]: tracing_core::Level
/// [targets]: tracing_core::Metadata::target
pub trait MakeWriter<'a> {
    /// The concrete [`io::Write`] implementation returned by [`make_writer`].
    ///
    /// [`io::Write`]: mycelium_util::io::Write
    /// [`make_writer`]: MakeWriter::make_writer
    type Writer: io::Write;

    /// Returns an instance of [`Writer`].
    ///
    /// # Implementer notes
    ///
    /// [`fmt::Subscriber`] or [`fmt::Collector`] will call this method each
    /// time an event is recorded. Ensure any state that must be saved across
    /// writes is not lost when the [`Writer`] instance is dropped. If creating
    /// a [`io::Write`] instance is expensive, be sure to cache it when
    /// implementing [`MakeWriter`] to improve performance.
    ///
    /// [`Writer`]: MakeWriter::Writer
    /// [`fmt::Subscriber`]: super::super::fmt::Subscriber
    /// [`fmt::Collector`]: super::super::fmt::Collector
    /// [`io::Write`]: mycelium_util::io::Write
    fn make_writer(&'a self) -> Self::Writer;

    /// Returns a [`Writer`] for writing data from the span or event described
    /// by the provided [`Metadata`].
    ///
    /// By default, this calls [`self.make_writer()`][make_writer], ignoring
    /// the provided metadata, but implementations can override this to provide
    /// metadata-specific behaviors.
    ///
    /// This method allows `MakeWriter` implementations to implement different
    /// behaviors based on the span or event being written. The `MakeWriter`
    /// type might return different writers based on the provided metadata, or
    /// might write some values to the writer before or after providing it to
    /// the caller.
    ///
    /// [`Writer`]: MakeWriter::Writer
    /// [`Metadata`]: tracing_core::Metadata
    /// [make_writer]: MakeWriter::make_writer
    /// [`WARN`]: tracing_core::Level::WARN
    /// [`ERROR`]: tracing_core::Level::ERROR
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        let _ = meta;
        self.make_writer()
    }
}

/// Extension trait adding combinators for working with types implementing
/// [`MakeWriter`].
///
/// This is not intended to be implemented directly for user-defined
/// [`MakeWriter`]s; instead, it should be imported when the desired methods are
/// used.
pub trait MakeWriterExt<'a>: MakeWriter<'a> {
    /// Wraps `self` and returns a [`MakeWriter`] that will only write output
    /// for events at or below the provided verbosity [`Level`]. For instance,
    /// `Level::TRACE` is considered to be _more verbose` than `Level::INFO`.
    ///
    /// Events whose level is more verbose than `level` will be ignored, and no
    /// output will be written.
    ///
    /// [`Level`]: tracing_core::Level
    /// [`io::Write`]: mycelium_util::io::Write
    fn with_max_level(self, level: tracing_core::Level) -> WithMaxLevel<Self>
    where
        Self: Sized,
    {
        WithMaxLevel::new(self, level)
    }

    /// Wraps `self` and returns a [`MakeWriter`] that will only write output
    /// for events at or above the provided verbosity [`Level`].
    ///
    /// Events whose level is less verbose than `level` will be ignored, and no
    /// output will be written.
    ///
    /// [`Level`]: tracing_core::Level
    /// [`io::Write`]: mycelium_util::io::Write
    fn with_min_level(self, level: tracing_core::Level) -> WithMinLevel<Self>
    where
        Self: Sized,
    {
        WithMinLevel::new(self, level)
    }

    /// Wraps `self` with a predicate that takes a span or event's [`Metadata`]
    /// and returns a `bool`. The returned [`MakeWriter`]'s
    /// [`MakeWriter::make_writer_for`][mwf] method will check the predicate to
    /// determine if  a writer should be produced for a given span or event.
    ///
    /// If the predicate returns `false`, the wrapped [`MakeWriter`]'s
    /// [`make_writer_for`][mwf] will return [`OptionalWriter::none`].
    /// Otherwise, it calls the wrapped [`MakeWriter`]'s
    /// [`make_writer_for`][mwf] method, and returns the produced writer.
    ///
    /// This can be used to filter an output based on arbitrary [`Metadata`]
    /// parameters.
    ///
    /// [`Metadata`]: tracing_core::Metadata
    /// [mwf]: MakeWriter::make_writer_for
    fn with_filter<F>(self, filter: F) -> WithFilter<Self, F>
    where
        Self: Sized,
        F: Fn(&Metadata<'_>) -> bool,
    {
        WithFilter::new(self, filter)
    }

    /// Combines `self` with another type implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that produces [writers] that write to *both*
    /// outputs.
    ///
    /// If writing to either writer returns an error, the returned writer will
    /// return that error. However, both writers will still be written to before
    /// the error is returned, so it is possible for one writer to fail while
    /// the other is written to successfully.
    ///
    /// [writers]: mycelium_util::io::Write
    fn and<B>(self, other: B) -> Tee<Self, B>
    where
        Self: Sized,
        B: MakeWriter<'a> + Sized,
    {
        Tee::new(self, other)
    }

    /// Combines `self` with another type implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that calls `other`'s [`make_writer`] if `self`'s
    /// `make_writer` returns [`OptionalWriter::none`].
    ///
    /// [`make_writer`]: MakeWriter::make_writer
    fn or_else<W, B>(self, other: B) -> OrElse<Self, B>
    where
        Self: MakeWriter<'a, Writer = OptionalWriter<W>> + Sized,
        B: MakeWriter<'a> + Sized,
        W: Write,
    {
        OrElse::new(self, other)
    }
}

/// A type implementing [`io::Write`] for a [`MutexGuard`] where the type
/// inside the [`Mutex`] implements [`io::Write`].
///
/// This is used by the [`MakeWriter`] implementation for [`Mutex`], because
/// [`MutexGuard`] itself will not implement [`io::Write`] — instead, it
/// _dereferences_ to a type implementing [`io::Write`]. Because [`MakeWriter`]
/// requires the `Writer` type to implement [`io::Write`], it's necessary to add
/// a newtype that forwards the trait implementation.
///
/// [`io::Write`]: mycelium_util::io::Write
/// [`MutexGuard`]: mycelium_util::sync::spin::MutexGuard
/// [`Mutex`]: mycelium_util::sync::spin::Mutex
/// [`MakeWriter`]: trait.MakeWriter.html
#[derive(Debug)]
pub struct MutexGuardWriter<'a, W>(MutexGuard<'a, W>);

// TODO(eliza): put this back if needed
/*
/// A writer that erases the specific [`io::Write`] and [`MakeWriter`] types being used.
///
/// This is useful in cases where the concrete type of the writer cannot be known
/// until runtime.
///
/// # Examples
///
/// A function that returns a [`Collect`] that will write to either stdout or stderr:
///
/// ```rust
/// # use tracing::Collect;
/// # use tracing_subscriber::fmt::writer::BoxMakeWriter;
///
/// fn dynamic_writer(use_stderr: bool) -> impl Collect {
///     let writer = if use_stderr {
///         BoxMakeWriter::new(core::io::stderr)
///     } else {
///         BoxMakeWriter::new(core::io::stdout)
///     };
///
///     tracing_subscriber::fmt().with_writer(writer).finish()
/// }
/// ```
///
/// [`Collect`]: tracing::Collect
/// [`io::Write`]: mycelium_util::io::Write
pub struct BoxMakeWriter {
    inner: Box<dyn for<'a> MakeWriter<'a, Writer = Box<dyn Write + 'a>> + Send + Sync>,
    name: &'static str,
}
 */

/// A [writer] that is one of two types implementing [`io::Write`][writer].
///
/// This may be used by [`MakeWriter`] implementations that may conditionally
/// return one of two writers.
///
/// [writer]: mycelium_util::io::Write
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EitherWriter<A, B> {
    /// A writer of type `A`.
    A(A),
    /// A writer of type `B`.
    B(B),
}

/// A [writer] which may or may not be enabled.
///
/// This may be used by [`MakeWriter`] implementations that wish to
/// conditionally enable or disable the returned writer based on a span or
/// event's [`Metadata`].
///
/// [writer]: mycelium_util::io::Write
pub type OptionalWriter<T> = EitherWriter<T, io::Sink>;

/// A [`MakeWriter`] combinator that only returns an enabled [writer] for spans
/// and events with metadata at or below a specified verbosity [`Level`].
///
/// This is returned by the [`MakeWriterExt::with_max_level] method. See the
/// method documentation for details.
///
/// [writer]: mycelium_util::io::Write
/// [`Level`]: tracing_core::Level
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithMaxLevel<M> {
    make: M,
    level: tracing_core::Level,
}

/// A [`MakeWriter`] combinator that only returns an enabled [writer] for spans
/// and events with metadata at or above a specified verbosity [`Level`].
///
/// This is returned by the [`MakeWriterExt::with_min_level] method. See the
/// method documentation for details.
///
/// [writer]: mycelium_util::io::Write
/// [`Level`]: tracing_core::Level
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithMinLevel<M> {
    make: M,
    level: tracing_core::Level,
}

/// A [`MakeWriter`] combinator that wraps a [`MakeWriter`] with a predicate for
/// span and event [`Metadata`], so that the [`MakeWriter::make_writer_for`]
/// method returns [`OptionalWriter::some`] when the predicate returns `true`,
/// and [`OptionalWriter::none`] when the predicate returns `false`.
///
/// This is returned by the [`MakeWriterExt::with_filter`] method. See the
/// method documentation for details.
///
/// [`Metadata`]: tracing_core::Metadata
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithFilter<M, F> {
    make: M,
    filter: F,
}

/// Combines a [`MakeWriter`] that returns an [`OptionalWriter`] with another
/// [`MakeWriter`], so that the second [`MakeWriter`] is used when the first
/// [`MakeWriter`] returns [`OptionalWriter::none`].
///
/// This is returned by the [`MakeWriterExt::or_else] method. See the
/// method documentation for details.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OrElse<A, B> {
    inner: A,
    or_else: B,
}

/// Combines two types implementing [`MakeWriter`] (or [`mycelium_util::io::Write`]) to
/// produce a writer that writes to both [`MakeWriter`]'s returned writers.
///
/// This is returned by the [`MakeWriterExt::and`] method. See the method
/// documentation for details.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Tee<A, B> {
    a: A,
    b: B,
}

impl<'a, F, W> MakeWriter<'a> for F
where
    F: Fn() -> W,
    W: io::Write,
{
    type Writer = W;

    fn make_writer(&'a self) -> Self::Writer {
        (self)()
    }
}
/*

// === impl BoxMakeWriter ===

impl BoxMakeWriter {
    /// Constructs a `BoxMakeWriter` wrapping a type implementing [`MakeWriter`].
    ///
    pub fn new<M>(make_writer: M) -> Self
    where
        M: for<'a> MakeWriter<'a> + Send + Sync + 'static,
    {
        Self {
            inner: Box::new(Boxed(make_writer)),
            name: core::any::type_name::<M>(),
        }
    }
}

impl Debug for BoxMakeWriter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("BoxMakeWriter")
            .field(&format_args!("<{}>", self.name))
            .finish()
    }
}

impl<'a> MakeWriter<'a> for BoxMakeWriter {
    type Writer = Box<dyn Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        self.inner.make_writer()
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        self.inner.make_writer_for(meta)
    }
}

struct Boxed<M>(M);

impl<'a, M> MakeWriter<'a> for Boxed<M>
where
    M: MakeWriter<'a>,
{
    type Writer = Box<dyn Write + 'a>;

    fn make_writer(&'a self) -> Self::Writer {
        let w = self.0.make_writer();
        Box::new(w)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        let w = self.0.make_writer_for(meta);
        Box::new(w)
    }
}
*/

// === impl Mutex/MutexGuardWriter ===

impl<'a, W> MakeWriter<'a> for Mutex<W>
where
    W: io::Write + 'a,
{
    type Writer = MutexGuardWriter<'a, W>;

    fn make_writer(&'a self) -> Self::Writer {
        MutexGuardWriter(self.lock())
    }
}

impl<'a, W> io::Write for MutexGuardWriter<'a, W>
where
    W: io::Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.write(buf)
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        self.0.write_all(buf)
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        self.0.write_fmt(fmt)
    }
}

// === impl EitherWriter ===

impl<A, B> io::Write for EitherWriter<A, B>
where
    A: io::Write,
    B: io::Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        match self {
            EitherWriter::A(a) => a.write(buf),
            EitherWriter::B(b) => b.write(buf),
        }
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        match self {
            EitherWriter::A(a) => a.flush(),
            EitherWriter::B(b) => b.flush(),
        }
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        match self {
            EitherWriter::A(a) => a.write_all(buf),
            EitherWriter::B(b) => b.write_all(buf),
        }
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        match self {
            EitherWriter::A(a) => a.write_fmt(fmt),
            EitherWriter::B(b) => b.write_fmt(fmt),
        }
    }
}

impl<T> OptionalWriter<T> {
    /// Returns a [disabled writer].
    ///
    /// Any bytes written to the returned writer are discarded.
    ///
    /// This is equivalent to returning [`Option::None`].
    ///
    /// [disabled writer]: mycelium_util::io::sink
    #[inline]
    pub fn none() -> Self {
        EitherWriter::B(mycelium_util::io::sink())
    }

    /// Returns an enabled writer of type `T`.
    ///
    /// This is equivalent to returning [`Option::Some`].
    #[inline]
    pub fn some(t: T) -> Self {
        EitherWriter::A(t)
    }
}

impl<T> From<Option<T>> for OptionalWriter<T> {
    #[inline]
    fn from(opt: Option<T>) -> Self {
        match opt {
            Some(writer) => Self::some(writer),
            None => Self::none(),
        }
    }
}

// === impl WithMaxLevel ===

impl<M> WithMaxLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a maximum [`Level`], so that it
    /// returns [`OptionalWriter::none`] for spans and events whose level is
    /// more verbose than the maximum level.
    ///
    /// See [`MakeWriterExt::with_max_level`] for details.
    ///
    /// [`Level`]: tracing_core::Level
    pub fn new(make: M, level: tracing_core::Level) -> Self {
        Self { make, level }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for WithMaxLevel<M> {
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        // If we don't know the level, assume it's disabled.
        OptionalWriter::none()
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if meta.level() <= &self.level {
            return OptionalWriter::some(self.make.make_writer_for(meta));
        }
        OptionalWriter::none()
    }
}

// === impl WithMinLevel ===

impl<M> WithMinLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a minimum [`Level`], so that it
    /// returns [`OptionalWriter::none`] for spans and events whose level is
    /// less verbose than the maximum level.
    ///
    /// See [`MakeWriterExt::with_min_level`] for details.
    ///
    /// [`Level`]: tracing_core::Level
    pub fn new(make: M, level: tracing_core::Level) -> Self {
        Self { make, level }
    }
}

impl<'a, M: MakeWriter<'a>> MakeWriter<'a> for WithMinLevel<M> {
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        // If we don't know the level, assume it's disabled.
        OptionalWriter::none()
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if meta.level() >= &self.level {
            return OptionalWriter::some(self.make.make_writer_for(meta));
        }
        OptionalWriter::none()
    }
}

// ==== impl WithFilter ===

impl<M, F> WithFilter<M, F> {
    /// Wraps `make` with the provided `filter`, returning a [`MakeWriter`] that
    /// will call `make.make_writer_for()` when `filter` returns `true` for a
    /// span or event's [`Metadata`], and returns a [`sink`] otherwise.
    ///
    /// See [`MakeWriterExt::with_filter`] for details.
    ///
    /// [`Metadata`]: tracing_core::Metadata
    /// [`sink`]: core::io::sink
    pub fn new(make: M, filter: F) -> Self
    where
        F: Fn(&Metadata<'_>) -> bool,
    {
        Self { make, filter }
    }
}

impl<'a, M, F> MakeWriter<'a> for WithFilter<M, F>
where
    M: MakeWriter<'a>,
    F: Fn(&Metadata<'_>) -> bool,
{
    type Writer = OptionalWriter<M::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        OptionalWriter::some(self.make.make_writer())
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        if (self.filter)(meta) {
            OptionalWriter::some(self.make.make_writer_for(meta))
        } else {
            OptionalWriter::none()
        }
    }
}

// === impl Tee ===

impl<A, B> Tee<A, B> {
    /// Combines two types implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that produces [writers] that write to *both*
    /// outputs.
    ///
    /// See the documentation for [`MakeWriterExt::and`] for details.
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

impl<'a, A, B> MakeWriter<'a> for Tee<A, B>
where
    A: MakeWriter<'a>,
    B: MakeWriter<'a>,
{
    type Writer = Tee<A::Writer, B::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        Tee::new(self.a.make_writer(), self.b.make_writer())
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        Tee::new(self.a.make_writer_for(meta), self.b.make_writer_for(meta))
    }
}

macro_rules! impl_tee {
    ($self_:ident.$f:ident($($arg:ident),*)) => {
        {
            let res_a = $self_.a.$f($($arg),*);
            let res_b = $self_.b.$f($($arg),*);
            (res_a?, res_b?)
        }
    }
}

impl<A, B> io::Write for Tee<A, B>
where
    A: io::Write,
    B: io::Write,
{
    #[inline]
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let (a, b) = impl_tee!(self.write(buf));
        Ok(core::cmp::max(a, b))
    }

    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        impl_tee!(self.flush());
        Ok(())
    }

    #[inline]
    fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
        impl_tee!(self.write_all(buf));
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
        impl_tee!(self.write_fmt(fmt));
        Ok(())
    }
}

// === impl OrElse ===

impl<A, B> OrElse<A, B> {
    /// Combines
    pub fn new<'a, W>(inner: A, or_else: B) -> Self
    where
        A: MakeWriter<'a, Writer = OptionalWriter<W>>,
        B: MakeWriter<'a>,
        W: Write,
    {
        Self { inner, or_else }
    }
}

impl<'a, A, B, W> MakeWriter<'a> for OrElse<A, B>
where
    A: MakeWriter<'a, Writer = OptionalWriter<W>>,
    B: MakeWriter<'a>,
    W: io::Write,
{
    type Writer = EitherWriter<W, B::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        match self.inner.make_writer() {
            EitherWriter::A(writer) => EitherWriter::A(writer),
            EitherWriter::B(_) => EitherWriter::B(self.or_else.make_writer()),
        }
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
        match self.inner.make_writer_for(meta) {
            EitherWriter::A(writer) => EitherWriter::A(writer),
            EitherWriter::B(_) => EitherWriter::B(self.or_else.make_writer_for(meta)),
        }
    }
}

// === blanket impls ===

impl<'a, M> MakeWriterExt<'a> for M where M: MakeWriter<'a> {}
