use std::borrow::Borrow;
use std::convert::AsRef;
use std::fmt;
use std::hash::{Hash, Hasher};
use std::ptr::NonNull;

/// Reference to an element of the set. Only used internal.
///
/// This is used for building a self-referential map inside the set.
#[derive(Copy, Clone, Eq, PartialEq)]
pub struct InternalRef<T: ?Sized>(NonNull<T>);

impl<T> InternalRef<T> {
    /// Build an `InternalRef` from a reference.
    ///
    /// # Warning
    ///
    /// An `InternalRef` removes the reference's lifetime. This is necessary
    /// to build the self-referential hash-to-index-map of the `IndexedHashSet`.
    /// Therefore, it is important to check the usage of `InternalRef`
    /// carefully to avoid use-after-free bugs!
    pub fn from_ref(t: &T) -> Self {
        InternalRef(t.into())
    }
}

impl<T: fmt::Debug> fmt::Debug for InternalRef<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "InternalRef to {:?}", self.as_ref())
    }
}

impl<T: ?Sized> AsRef<T> for InternalRef<T> {
    /// Get a normal reference from the `InternalRef`.
    ///
    /// # Unsafe
    ///
    /// The returned reference's lifetime is not tied to the original value.
    /// If the origin of the `InternalRef` has been moved since creation of the
    /// `InternalRef` this method invokes **Undefined Behavior**.
    fn as_ref(&self) -> &T {
        unsafe { self.0.as_ref() }
    }
}

impl<T> Hash for InternalRef<T>
where
    T: Hash,
{
    /// Hashes the referenced `T` just like a normal reference.
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_ref().hash(state)
    }
}

/// A helper struct to allow using `InternalRef` as key in a `HashSet`
///
/// # Details
///
/// Original code from @jethrogb in
/// https://users.rust-lang.org/t/simulating-a-hashmap-k-v-where-the-key-is-not-k/48028/
///
/// This allows to build an API for the set similar to the APIs of `HashSet`
/// and `HashMap`. However, this prevents us from implementing `Borrow<T>` for
/// `InternalRef<T>`. Instead we must always use a `BorrowWrapper` which does
/// not hurt to much as this _new type_ should be optimized away at compile
/// time.
#[derive(Debug, Hash, Eq, PartialEq)]
#[repr(transparent)]
pub struct BorrowWrapper<T: ?Sized>(T);

impl<T: ?Sized> BorrowWrapper<T> {
    /// Construct a wrapper.
    pub fn from_ref(r: &T) -> &Self {
        unsafe { &*(r as *const T as *const BorrowWrapper<T>) }
    }
}

impl<T, Q> Borrow<BorrowWrapper<Q>> for InternalRef<T>
where
    T: ?Sized + Borrow<Q>,
    Q: ?Sized,
{
    fn borrow(&self) -> &BorrowWrapper<Q> {
        BorrowWrapper::from_ref(self.as_ref().borrow())
    }
}

pub trait Wrap {
    fn wrap(&self) -> &BorrowWrapper<Self>;
}

impl<T: ?Sized> Wrap for T {
    fn wrap(&self) -> &BorrowWrapper<Self> {
        BorrowWrapper::from_ref(self)
    }
}
