use std::{
    alloc,
    borrow::Borrow,
    fmt, hash,
    mem::{size_of, ManuallyDrop},
    ptr::{self, NonNull},
    slice, str,
};

/// A very very small owned string type.
///
/// This type is like a `Box<str>` but can only fit strings with a byte length smaller than 256.
/// On 64-bit machines this type stores up to 15 bytes inline (7 bytes on 32-bit machines). One
/// byte is used to store the length. For strings short enough to be stored inline, the remaining
/// 15 (or 7) bytes store the content inline. Otherwise the second `usize` of memory is a thin
/// pointer to the string content.
///
/// Unlike `Box<str>` this type is not null-pointer optimized.
#[repr(C)]
pub struct TinyBoxedStr {
    len: u8,
    prefix: [u8; Self::PREFIX_LEN],
    trailing: TinyStrTrailing,
}

#[repr(C)]
union TinyStrTrailing {
    suffix: [u8; TinyBoxedStr::SUFFIX_LEN],
    ptr: ManuallyDrop<NonNull<u8>>,
}

impl TinyBoxedStr {
    const PREFIX_LEN: usize = size_of::<usize>() - size_of::<u8>();
    const SUFFIX_LEN: usize = size_of::<usize>();
    const INLINE_LEN: u8 = (Self::PREFIX_LEN + Self::SUFFIX_LEN) as u8;

    pub const MAX_LEN: usize = u8::MAX as usize;

    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_bytes(&self) -> &[u8] {
        let ptr = if self.len <= Self::INLINE_LEN {
            let ptr = ptr::from_ref(self);
            unsafe { ptr::addr_of!((*ptr).prefix) }.cast()
        } else {
            unsafe { self.trailing.ptr }.as_ptr()
        };
        unsafe { slice::from_raw_parts(ptr, self.len()) }
    }

    #[inline]
    pub fn as_str(&self) -> &str {
        unsafe { str::from_utf8_unchecked(self.as_bytes()) }
    }

    fn layout(len: usize) -> alloc::Layout {
        alloc::Layout::array::<u8>(len)
            .expect("a valid layout for an array")
            .pad_to_align()
    }

    fn copy_bytes(source: &[u8]) -> NonNull<u8> {
        let layout = Self::layout(source.len());
        let nullable = unsafe { alloc::alloc(layout) };
        let ptr = match NonNull::new(nullable) {
            Some(ptr) => ptr.cast(),
            None => alloc::handle_alloc_error(layout),
        };
        unsafe {
            ptr::copy_nonoverlapping(source.as_ptr(), ptr.as_ptr(), source.len());
        }
        ptr
    }
}

#[derive(Debug)]
pub struct TooLongError;

impl TryFrom<&str> for TinyBoxedStr {
    type Error = TooLongError;

    fn try_from(s: &str) -> Result<Self, Self::Error> {
        if s.len() > Self::MAX_LEN {
            return Err(TooLongError);
        }

        let len = s.len() as u8;
        let bytes = s.as_bytes();
        let mut prefix = [0; Self::PREFIX_LEN];
        let trailing = if len <= Self::INLINE_LEN {
            let mut suffix = [0; Self::SUFFIX_LEN];
            if s.len() <= Self::PREFIX_LEN {
                prefix[..s.len()].copy_from_slice(bytes);
            } else {
                prefix.copy_from_slice(&bytes[..Self::PREFIX_LEN]);
                suffix[..s.len() - Self::PREFIX_LEN].copy_from_slice(&bytes[Self::PREFIX_LEN..]);
            }
            TinyStrTrailing { suffix }
        } else {
            prefix.copy_from_slice(&bytes[..Self::PREFIX_LEN]);
            let ptr = ManuallyDrop::new(Self::copy_bytes(bytes));
            TinyStrTrailing { ptr }
        };

        Ok(Self {
            len,
            prefix,
            trailing,
        })
    }
}

impl Drop for TinyBoxedStr {
    fn drop(&mut self) {
        if self.len > Self::INLINE_LEN {
            let ptr = unsafe { self.trailing.ptr }.as_ptr();
            let layout = Self::layout(self.len());
            unsafe { alloc::dealloc(ptr.cast(), layout) }
        }
    }
}

impl Clone for TinyBoxedStr {
    fn clone(&self) -> Self {
        let trailing = if self.len <= Self::INLINE_LEN {
            let suffix = unsafe { self.trailing.suffix };
            TinyStrTrailing { suffix }
        } else {
            let ptr = ManuallyDrop::new(Self::copy_bytes(self.as_bytes()));
            TinyStrTrailing { ptr }
        };

        Self {
            len: self.len,
            prefix: self.prefix,
            trailing,
        }
    }
}

impl Default for TinyBoxedStr {
    fn default() -> Self {
        Self {
            len: 0,
            prefix: [0; Self::PREFIX_LEN],
            trailing: TinyStrTrailing {
                suffix: [0; Self::SUFFIX_LEN],
            },
        }
    }
}

impl AsRef<str> for TinyBoxedStr {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for TinyBoxedStr {
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

// NOTE: this could be specialized to optimize the number of comparison operations. We could cast
// the first `usize` of memory together to do a single comparison (and same for the suffixes).
// This optimization would only matter if we compared these strings very frequently however.
impl PartialEq for TinyBoxedStr {
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for TinyBoxedStr {}

impl PartialEq<str> for TinyBoxedStr {
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl hash::Hash for TinyBoxedStr {
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl fmt::Debug for TinyBoxedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

impl fmt::Display for TinyBoxedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.as_str().fmt(f)
    }
}

unsafe impl Send for TinyBoxedStr {}
unsafe impl Sync for TinyBoxedStr {}
