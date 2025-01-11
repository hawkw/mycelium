//! Abstractions for creating [`fmt::Write`] instances.
//!
//! [`fmt::Write`]: mycelium_util::fmt::Write
use crate::color::{Color, SetColor};
use maitake::sync::{
    blocking::{Mutex, MutexGuard, RawMutex},
    spin::Spinlock,
};
use mycelium_util::fmt::{self, Debug};
use tracing_core::Metadata;

/// A type that can create [`fmt::Write`] instances.
///
/// This trait is already implemented for function pointers and
/// immutably-borrowing closures that return an instance of [`fmt::Write`],
/// Additionally, it is implemented for [`maitake::sync::blocking::Mutex`]
/// when the type inside the mutex implements [`fmt::Write`] and the `Lock` type
/// implements [`RawMutex`].
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
/// [`fmt::Write`]: mycelium_util::fmt::Write
/// [`Event`]: tracing_core::event::Event
/// [`MakeWriter::make_writer_for`]: MakeWriter::make_writer_for
/// [`Metadata`]: tracing_core::Metadata
/// [levels]: tracing_core::Level
/// [targets]: tracing_core::Metadata::target
pub trait MakeWriter<'a> {
    /// The concrete [`fmt::Write`] implementation returned by [`make_writer`].
    ///
    /// [`fmt::Write`]: mycelium_util::fmt::Write
    /// [`make_writer`]: MakeWriter::make_writer
    type Writer: fmt::Write;

    /// Returns an instance of [`Writer`].
    ///
    /// # Implementer notes
    ///
    /// A [`Subscriber`] will call this method each  time an event is recorded.
    /// Ensure any state that must be saved across writes is not lost when the
    /// [`Writer`] instance is dropped. If creating a [`fmt::Write`] instance is
    /// expensive, be sure to cache it when implementing [`MakeWriter`] to
    /// improve performance.
    ///
    /// [`Writer`]: MakeWriter::Writer
    /// [`Subscriber`]: crate::Subscriber
    /// [`fmt::Write`]: mycelium_util::fmt::Write
    fn make_writer(&'a self) -> Self::Writer;

    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        let _ = meta;
        true
    }

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
    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        if self.enabled(meta) {
            return Some(self.make_writer());
        }

        None
    }

    fn line_len(&self) -> usize {
        80
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
    /// [`fmt::Write`]: mycelium_util::fmt::Write
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
    /// [`fmt::Write`]: mycelium_util::fmt::Write
    fn with_min_level(self, level: tracing_core::Level) -> WithMinLevel<Self>
    where
        Self: Sized,
    {
        WithMinLevel::new(self, level)
    }

    /// Wraps `self` with a predicate that takes a span or event's [`Metadata`]
    /// and returns a `bool`. The returned [`MakeWriter`]'s
    /// [`MakeWriter::make_writer_for`] method will check the predicate to
    /// determine if  a writer should be produced for a given span or event.
    ///
    /// If the predicate returns `false`, the wrapped [`MakeWriter`]'s
    /// [`make_writer_for`][mwf] will return [`None`].
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
    /// [writers]: mycelium_util::fmt::Write
    fn and<B>(self, other: B) -> Tee<Self, B>
    where
        Self: Sized,
        B: MakeWriter<'a> + Sized,
    {
        Tee::new(self, other)
    }

    /// Combines `self` with another type implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that calls `other`'s [`make_writer`] if `self`'s
    /// `make_writer` returns [`None`].
    ///
    /// [`make_writer`]: MakeWriter::make_writer
    fn or_else<B>(self, other: B) -> OrElse<Self, B>
    where
        Self: MakeWriter<'a> + Sized,
        B: MakeWriter<'a> + Sized,
    {
        OrElse::new(self, other)
    }

    fn with_line_len(self, len: usize) -> WithLineLen<Self>
    where
        Self: MakeWriter<'a> + Sized,
    {
        WithLineLen::new(self, len)
    }
}

/// A type implementing [`fmt::Write`] for a [`MutexGuard`] where the type
/// inside the [`Mutex`] implements [`fmt::Write`].
///
/// This is used by the [`MakeWriter`] implementation for [`Mutex`], because
/// [`MutexGuard`] itself will not implement [`fmt::Write`] — instead, it
/// _dereferences_ to a type implementing [`fmt::Write`]. Because [`MakeWriter`]
/// requires the `Writer` type to implement [`fmt::Write`], it's necessary to add
/// a newtype that forwards the trait implementation.
///
/// [`fmt::Write`]: mycelium_util::fmt::Write
/// [`MutexGuard`]: maitake::sync::blocking::MutexGuard
/// [`Mutex`]: maitake::sync::blocking::Mutex
/// [`MakeWriter`]: trait.MakeWriter.html
#[derive(Debug)]
pub struct MutexGuardWriter<'a, W, Lock: RawMutex = Spinlock>(MutexGuard<'a, W, Lock>);

// TODO(eliza): put this back if needed
/*
/// A writer that erases the specific [`fmt::Write`] and [`MakeWriter`] types being used.
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
/// [`fmt::Write`]: mycelium_util::fmt::Write
pub struct BoxMakeWriter {
    inner: Box<dyn for<'a> MakeWriter<'a, Writer = Box<dyn Write + 'a>> + Send + Sync>,
    name: &'static str,
}
 */

/// A [writer] that is one of two types implementing [`fmt::Write`].
///
/// This may be used by [`MakeWriter`] implementations that may conditionally
/// return one of two writers.
///
/// [writer]: mycelium_util::fmt::Write
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub enum EitherWriter<A, B> {
    /// A writer of type `A`.
    A(A),
    /// A writer of type `B`.
    B(B),
}

/// A [`MakeWriter`] combinator that only returns an enabled [writer] for spans
/// and events with metadata at or below a specified verbosity [`Level`].
///
/// This is returned by the [`MakeWriterExt::with_max_level] method. See the
/// method documentation for details.
///
/// [writer]: mycelium_util::fmt::Write
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
/// [writer]: mycelium_util::fmt::Write
/// [`Level`]: tracing_core::Level
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithMinLevel<M> {
    make: M,
    level: tracing_core::Level,
}

/// A [`MakeWriter`] combinator that wraps a [`MakeWriter`] with a predicate for
/// span and event [`Metadata`], so that the [`MakeWriter::make_writer_for`]
/// method returns [`Some`] when the predicate returns `true`,
/// and [`None`] when the predicate returns `false`.
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

/// Combines a [`MakeWriter`] that returns an [`Option`] of another
/// [`MakeWriter`], so that the second [`MakeWriter`] is used when the first
/// [`MakeWriter`] returns [`None`].
///
/// This is returned by the [`MakeWriterExt::or_else] method. See the
/// method documentation for details.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct OrElse<A, B> {
    inner: A,
    or_else: B,
}

/// Combines two types implementing [`MakeWriter`] (or [`mycelium_util::fmt::Write`]) to
/// produce a writer that writes to both [`MakeWriter`]'s returned writers.
///
/// This is returned by the [`MakeWriterExt::and`] method. See the method
/// documentation for details.
#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct Tee<A, B> {
    a: A,
    b: B,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct WithLineLen<W> {
    make: W,
    len: usize,
}

#[derive(Copy, Clone, Debug, Eq, PartialEq, Default)]
pub struct NoWriter(());

pub const fn none() -> NoWriter {
    NoWriter(())
}

impl<'a, F, W> MakeWriter<'a> for F
where
    F: Fn() -> W,
    W: fmt::Write,
{
    type Writer = W;

    fn make_writer(&'a self) -> Self::Writer {
        (self)()
    }
}

impl<'a, M> MakeWriter<'a> for Option<M>
where
    M: MakeWriter<'a>,
{
    type Writer = EitherWriter<M::Writer, NoWriter>;
    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        self.as_ref()
            .map(MakeWriter::make_writer)
            .map(EitherWriter::A)
            .unwrap_or(EitherWriter::B(NoWriter(())))
    }

    #[inline]
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        self.as_ref()
            .map(|make| make.enabled(meta))
            .unwrap_or(false)
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        self.as_ref()
            .and_then(|make| make.make_writer_for(meta))
            .map(EitherWriter::A)
    }

    #[inline]
    fn line_len(&self) -> usize {
        self.as_ref().map(MakeWriter::line_len).unwrap_or(80)
    }
}

// === impl Mutex/MutexGuardWriter ===

impl<'a, W, Lock> MakeWriter<'a> for Mutex<W, Lock>
where
    W: fmt::Write + 'a,
    Lock: RawMutex + 'a,
{
    type Writer = MutexGuardWriter<'a, W, Lock>;

    fn make_writer(&'a self) -> Self::Writer {
        MutexGuardWriter(self.lock())
    }
}

impl<W, Lock> fmt::Write for MutexGuardWriter<'_, W, Lock>
where
    W: fmt::Write,
    Lock: RawMutex,
{
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.0.write_str(s)
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> fmt::Result {
        self.0.write_fmt(fmt)
    }
}

// === impl EitherWriter ===

impl<A, B> fmt::Write for EitherWriter<A, B>
where
    A: fmt::Write,
    B: fmt::Write,
{
    #[inline]
    fn write_str(&mut self, s: &str) -> fmt::Result {
        match self {
            EitherWriter::A(a) => a.write_str(s),
            EitherWriter::B(b) => b.write_str(s),
        }
    }

    #[inline]
    fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> fmt::Result {
        match self {
            EitherWriter::A(a) => a.write_fmt(fmt),
            EitherWriter::B(b) => b.write_fmt(fmt),
        }
    }
}

impl<A, B> SetColor for EitherWriter<A, B>
where
    A: fmt::Write + SetColor,
    B: fmt::Write + SetColor,
{
    fn set_fg_color(&mut self, color: Color) {
        match self {
            EitherWriter::A(a) => a.set_fg_color(color),
            EitherWriter::B(b) => b.set_fg_color(color),
        }
    }

    fn fg_color(&self) -> Color {
        match self {
            EitherWriter::A(a) => a.fg_color(),
            EitherWriter::B(b) => b.fg_color(),
        }
    }

    fn set_bold(&mut self, bold: bool) {
        match self {
            EitherWriter::A(a) => a.set_bold(bold),
            EitherWriter::B(b) => b.set_bold(bold),
        }
    }
}

// === impl WithMaxLevel ===

impl<M> WithMaxLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a maximum [`Level`], so that it
    /// returns [`None`] for spans and events whose level is
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
    type Writer = M::Writer;

    #[inline(always)]
    fn make_writer(&'a self) -> Self::Writer {
        self.make.make_writer()
    }

    #[inline(always)]
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        meta.level() <= &self.level && self.make.enabled(meta)
    }

    #[inline(always)]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        if self.enabled(meta) {
            return self.make.make_writer_for(meta);
        }

        None
    }

    #[inline(always)]
    fn line_len(&self) -> usize {
        self.make.line_len()
    }
}

// === impl WithMinLevel ===

impl<M> WithMinLevel<M> {
    /// Wraps the provided [`MakeWriter`] with a minimum [`Level`], so that it
    /// returns [`None`] for spans and events whose level is
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
    type Writer = M::Writer;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        self.make.make_writer()
    }

    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        meta.level() >= &self.level && self.make.enabled(meta)
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        if self.enabled(meta) {
            return self.make.make_writer_for(meta);
        }

        None
    }

    #[inline]
    fn line_len(&self) -> usize {
        self.make.line_len()
    }
}

// ==== impl WithFilter ===

impl<M, F> WithFilter<M, F> {
    /// Wraps `make` with the provided `filter`, returning a [`MakeWriter`] that
    /// will call `make.make_writer_for()` when `filter` returns `true` for a
    /// span or event's [`Metadata`], and returns [`None`] otherwise.
    ///
    /// See [`MakeWriterExt::with_filter`] for details.
    ///
    /// [`Metadata`]: tracing_core::Metadata
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
    type Writer = M::Writer;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        self.make.make_writer()
    }

    #[inline]
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        (self.filter)(meta) && self.make.enabled(meta)
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        if self.enabled(meta) {
            return self.make.make_writer_for(meta);
        }

        None
    }

    #[inline]
    fn line_len(&self) -> usize {
        self.make.line_len()
    }
}

// === impl Tee ===

impl<A, B> Tee<A, B> {
    /// Combines two types implementing [`MakeWriter`], returning
    /// a new [`MakeWriter`] that produces [writers] that write to *both*
    /// outputs.
    ///
    /// See the documentation for [`MakeWriterExt::and`] for details.
    ///
    /// [writers]: fmt::Write
    pub fn new(a: A, b: B) -> Self {
        Self { a, b }
    }
}

// impl<'a, A, B> MakeWriter<'a> for Tee<A, B>
// where
//     A: MakeWriter<'a>,
//     B: MakeWriter<'a>,
// {
//     type Writer = Tee<A::Writer, B::Writer>;

//     #[inline]
//     fn make_writer(&'a self) -> Self::Writer {
//         Tee::new(self.a.make_writer(), self.b.make_writer())
//     }

//     #[inline]
//     fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Self::Writer {
//         Tee::new(self.a.make_writer_for(meta), self.b.make_writer_for(meta))
//     }
// }

// macro_rules! impl_tee {
//     ($self_:ident.$f:ident($($arg:ident),*)) => {
//         {
//             let res_a = $self_.a.$f($($arg),*);
//             let res_b = $self_.b.$f($($arg),*);
//             (res_a?, res_b?)
//         }
//     }
// }

// impl<A, B> fmt::Write for Tee<A, B>
// where
//     A: fmt::Write,
//     B: fmt::Write,
// {
//     #[inline]
//     fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
//         let (a, b) = impl_tee!(self.write(buf));
//         Ok(core::cmp::max(a, b))
//     }

//     #[inline]
//     fn flush(&mut self) -> io::Result<()> {
//         impl_tee!(self.flush());
//         Ok(())
//     }

//     #[inline]
//     fn write_all(&mut self, buf: &[u8]) -> io::Result<()> {
//         impl_tee!(self.write_all(buf));
//         Ok(())
//     }

//     #[inline]
//     fn write_fmt(&mut self, fmt: fmt::Arguments<'_>) -> io::Result<()> {
//         impl_tee!(self.write_fmt(fmt));
//         Ok(())
//     }
// }

// === impl OrElse ===

impl<A, B> OrElse<A, B> {
    /// Combines
    pub fn new<'a>(inner: A, or_else: B) -> Self
    where
        A: MakeWriter<'a>,
        B: MakeWriter<'a>,
    {
        Self { inner, or_else }
    }
}

impl<'a, A, B> MakeWriter<'a> for OrElse<A, B>
where
    A: MakeWriter<'a>,
    B: MakeWriter<'a>,
{
    type Writer = EitherWriter<A::Writer, B::Writer>;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        EitherWriter::A(self.inner.make_writer())
    }

    #[inline]
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        self.inner.enabled(meta) || self.or_else.enabled(meta)
    }

    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        self.inner
            .make_writer_for(meta)
            .map(EitherWriter::A)
            .or_else(|| self.or_else.make_writer_for(meta).map(EitherWriter::B))
    }

    #[inline]
    fn line_len(&self) -> usize {
        core::cmp::min(self.inner.line_len(), self.or_else.line_len())
    }
}

// === impl WithLineLen ===

impl<W> WithLineLen<W> {
    /// Combines
    pub fn new(make: W, len: usize) -> Self {
        Self { make, len }
    }
}

impl<'a, W> MakeWriter<'a> for WithLineLen<W>
where
    W: MakeWriter<'a>,
{
    type Writer = W::Writer;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        self.make.make_writer()
    }

    #[inline]
    fn enabled(&self, meta: &Metadata<'_>) -> bool {
        self.make.enabled(meta)
    }

    #[inline]
    fn make_writer_for(&'a self, meta: &Metadata<'_>) -> Option<Self::Writer> {
        self.make.make_writer_for(meta)
    }

    #[inline]
    fn line_len(&self) -> usize {
        self.len
    }
}

// === impl NoWriter ===

impl fmt::Write for NoWriter {
    #[inline]
    fn write_str(&mut self, _: &str) -> fmt::Result {
        Ok(())
    }

    #[inline]
    fn write_fmt(&mut self, _: fmt::Arguments<'_>) -> fmt::Result {
        Ok(())
    }
}

impl<'a> MakeWriter<'a> for NoWriter {
    type Writer = Self;

    #[inline]
    fn make_writer(&'a self) -> Self::Writer {
        Self(())
    }

    #[inline]
    fn enabled(&self, _: &Metadata<'_>) -> bool {
        false
    }

    #[inline]
    fn make_writer_for(&'a self, _: &Metadata<'_>) -> Option<Self::Writer> {
        None
    }
}

impl SetColor for NoWriter {
    fn set_fg_color(&mut self, _: Color) {
        // nop
    }

    fn fg_color(&self) -> Color {
        Color::Default
    }

    fn set_bold(&mut self, _: bool) {
        // nop
    }
}

// === blanket impls ===

impl<'a, M> MakeWriterExt<'a> for M where M: MakeWriter<'a> {}
