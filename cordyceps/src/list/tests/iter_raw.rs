use super::*;
use core::fmt::Debug;

/// This vtable is usable on type-erased nodes.
///
/// All functions take the type-erased versions of `NonNull<OwnedEntryNode<T>>`.
struct VTable {
    print: fn(NonNull<()>),
    drop: fn(NonNull<()>),
}

/// A helper function for creating a basic "print" function
fn print<T: Debug>(this: NonNull<()>) {
    let this: NonNull<OwnedEntryNode<T>> = this.cast();
    let this: &OwnedEntryNode<T> = unsafe { this.as_ref() };
    println!("PRINT: {:?}", this.item);
}

/// A helper function for creating a basic "drop" function
fn droppa<T>(this: NonNull<()>) {
    let this: NonNull<OwnedEntryNode<T>> = this.cast();
    unsafe {
        drop(Box::from_raw(this.as_ptr()));
    }
}

impl VTable {
    /// This can be used to create a vtable for a given type T
    pub const fn vtable_for<T: Debug>() -> Self {
        Self {
            print: print::<T>,
            drop: droppa::<T>,
        }
    }
}

/// This is the type erased common header of all `OwnedEntryNode` items.
#[pin_project::pin_project]
struct OwnedEntryHeader {
    #[pin]
    links: Links<OwnedEntryHeader>,
    vtable: &'static VTable,
}

// LOAD BEARING: this must be repr(c) to guarantee `hdr` is the first field
#[repr(C)]
struct OwnedEntryNode<T> {
    // LOAD BEARING: this must be the first element for type-punning to work
    hdr: OwnedEntryHeader,
    item: T,
}

/// A wrapper type that allows us to impl drop on a node using vtable methods
struct NodeRef(NonNull<OwnedEntryHeader>);

/// Convert a typed node into an untyped noderef
impl<T> From<Pin<Box<OwnedEntryNode<T>>>> for NodeRef {
    fn from(value: Pin<Box<OwnedEntryNode<T>>>) -> Self {
        let ptr: NonNull<OwnedEntryNode<T>> =
            unsafe { NonNull::from(Box::leak(Pin::into_inner_unchecked(value))) };
        let ptr: NonNull<OwnedEntryHeader> = ptr.cast();
        NodeRef(ptr)
    }
}

/// We need to use the vtable method for drop, because the types are otherwise
/// erased
impl Drop for NodeRef {
    fn drop(&mut self) {
        let ptr: NonNull<OwnedEntryHeader> = self.0;
        let vtable: &'static VTable = unsafe { ptr.as_ref().vtable };
        let ptr: NonNull<()> = ptr.cast();
        (vtable.drop)(ptr);
    }
}

unsafe impl Linked<Links<Self>> for OwnedEntryHeader {
    type Handle = NodeRef;

    fn into_ptr(handle: NodeRef) -> NonNull<Self> {
        let ptr = handle.0;
        core::mem::forget(handle);
        ptr
    }

    unsafe fn from_ptr(ptr: NonNull<Self>) -> Self::Handle {
        NodeRef(ptr)
    }

    unsafe fn links(target: NonNull<Self>) -> NonNull<Links<Self>> {
        let links = ptr::addr_of_mut!((*target.as_ptr()).links);
        NonNull::new_unchecked(links)
    }
}

/// Create a new typed node, that can be erased by calling `Into::into` to
/// a `NodeRef` type.
fn owned_entry<T: Debug>(val: T) -> Pin<Box<OwnedEntryNode<T>>> {
    Box::pin(OwnedEntryNode {
        hdr: OwnedEntryHeader {
            links: Links::new(),
            vtable: const { &VTable::vtable_for::<T>() },
        },
        item: val,
    })
}

/// Just to make sure miri doesn't get big mad about these whole shenanigans
#[test]
fn smoke() {
    let mut list = List::<OwnedEntryHeader>::new();
    let a = owned_entry(42i32);
    let b = owned_entry("bar");
    let c = owned_entry(Some(123u64));
    list.push_front(a.into());
    list.push_front(b.into());
    list.push_front(c.into());

    // We use iter_raw instead of iter_mut to allow type punning
    for node in list.iter_raw() {
        // obtain the vtable
        let vtable: &'static VTable = unsafe { node.as_ref().vtable };
        // type erase, call vtable method
        let node: NonNull<()> = node.cast();
        (vtable.print)(node);
    }

    // Try running this with `cargo +nightly miri test -- iter_raw --nocapture`, it's pretty neat :)

    drop(list);
}
