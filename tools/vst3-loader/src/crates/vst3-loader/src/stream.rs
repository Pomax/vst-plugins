//! An in-memory `IBStream`.
//!
//! A host has to hand the plugin somewhere to write its state. A DAW would use
//! a chunk of its project file; here it is a `Vec<u8>` we can inspect, which is
//! what lets a test assert on exactly what the plugin persisted.

use std::ffi::c_void;
use std::sync::Mutex;

use vst3::{Class, Steinberg::*};

pub struct MemoryStream {
    inner: Mutex<Inner>,
}

struct Inner {
    data: Vec<u8>,
    pos: usize,
}

impl MemoryStream {
    pub fn new() -> MemoryStream {
        MemoryStream {
            inner: Mutex::new(Inner {
                data: Vec::new(),
                pos: 0,
            }),
        }
    }

    pub fn with_data(data: Vec<u8>) -> MemoryStream {
        MemoryStream {
            inner: Mutex::new(Inner { data, pos: 0 }),
        }
    }

    /// A copy of everything written so far.
    pub fn data(&self) -> Vec<u8> {
        self.inner.lock().map(|i| i.data.clone()).unwrap_or_default()
    }

    /// Rewind, so the plugin reads from the beginning.
    pub fn rewind(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.pos = 0;
        }
    }
}

impl Default for MemoryStream {
    fn default() -> Self {
        MemoryStream::new()
    }
}

impl Class for MemoryStream {
    type Interfaces = (IBStream,);
}

impl IBStreamTrait for MemoryStream {
    unsafe fn read(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_read: *mut int32,
    ) -> tresult {
        if buffer.is_null() || num_bytes < 0 {
            return kInvalidArgument;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        let available = inner.data.len().saturating_sub(inner.pos);
        let n = available.min(num_bytes as usize);
        if n > 0 {
            std::ptr::copy_nonoverlapping(inner.data[inner.pos..].as_ptr(), buffer as *mut u8, n);
            inner.pos += n;
        }
        if !num_bytes_read.is_null() {
            *num_bytes_read = n as int32;
        }
        kResultOk
    }

    unsafe fn write(
        &self,
        buffer: *mut c_void,
        num_bytes: int32,
        num_bytes_written: *mut int32,
    ) -> tresult {
        if buffer.is_null() || num_bytes < 0 {
            return kInvalidArgument;
        }
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        let n = num_bytes as usize;
        let src = std::slice::from_raw_parts(buffer as *const u8, n);
        let pos = inner.pos;
        if pos + n > inner.data.len() {
            inner.data.resize(pos + n, 0);
        }
        inner.data[pos..pos + n].copy_from_slice(src);
        inner.pos += n;
        if !num_bytes_written.is_null() {
            *num_bytes_written = n as int32;
        }
        kResultOk
    }

    unsafe fn seek(&self, pos: int64, mode: int32, result: *mut int64) -> tresult {
        let Ok(mut inner) = self.inner.lock() else {
            return kInternalError;
        };
        let len = inner.data.len() as i64;

        // The generated seek-mode constants are `i32` on some platforms and
        // `u32` on others, so both sides are widened before comparing rather
        // than matched directly.
        let mode = mode as i64;
        let seek_set = IBStream_::IStreamSeekMode_::kIBSeekSet as i64;
        let seek_cur = IBStream_::IStreamSeekMode_::kIBSeekCur as i64;
        let seek_end = IBStream_::IStreamSeekMode_::kIBSeekEnd as i64;

        let base = if mode == seek_set {
            0
        } else if mode == seek_cur {
            inner.pos as i64
        } else if mode == seek_end {
            len
        } else {
            return kInvalidArgument;
        };
        let target = (base + pos).clamp(0, len);
        inner.pos = target as usize;
        if !result.is_null() {
            *result = target;
        }
        kResultOk
    }

    unsafe fn tell(&self, pos: *mut int64) -> tresult {
        let Ok(inner) = self.inner.lock() else {
            return kInternalError;
        };
        if !pos.is_null() {
            *pos = inner.pos as int64;
        }
        kResultOk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The stream as a plugin sees it: through the COM interface, not through
    /// the Rust methods beside it.
    fn write(stream: &MemoryStream, bytes: &[u8]) -> (tresult, i32) {
        let mut written: int32 = 0;
        let result = unsafe {
            stream.write(
                bytes.as_ptr() as *mut c_void,
                bytes.len() as int32,
                &mut written,
            )
        };
        (result, written)
    }

    fn read(stream: &MemoryStream, len: usize) -> (tresult, Vec<u8>) {
        let mut buffer = vec![0u8; len];
        let mut got: int32 = 0;
        let result = unsafe {
            stream.read(buffer.as_mut_ptr() as *mut c_void, len as int32, &mut got)
        };
        buffer.truncate(got.max(0) as usize);
        (result, buffer)
    }

    fn seek(stream: &MemoryStream, pos: i64, mode: i32) -> (tresult, i64) {
        let mut at: int64 = -1;
        let result = unsafe { stream.seek(pos, mode, &mut at) };
        (result, at)
    }

    const SET: i32 = IBStream_::IStreamSeekMode_::kIBSeekSet as i32;
    const CUR: i32 = IBStream_::IStreamSeekMode_::kIBSeekCur as i32;
    const END: i32 = IBStream_::IStreamSeekMode_::kIBSeekEnd as i32;

    #[test]
    fn what_is_written_can_be_read_back() {
        let stream = MemoryStream::new();
        let (result, written) = write(&stream, b"plugin state");
        assert_eq!(result, kResultOk);
        assert_eq!(written, 12);
        assert_eq!(stream.data(), b"plugin state");

        stream.rewind();
        let (result, bytes) = read(&stream, 12);
        assert_eq!(result, kResultOk);
        assert_eq!(bytes, b"plugin state");
    }

    #[test]
    fn reading_past_the_end_returns_what_is_there() {
        let stream = MemoryStream::with_data(b"short".to_vec());
        let (result, bytes) = read(&stream, 100);
        assert_eq!(result, kResultOk, "a short read is not an error");
        assert_eq!(bytes, b"short");

        // And again, with nothing left.
        let (result, bytes) = read(&stream, 100);
        assert_eq!(result, kResultOk);
        assert!(bytes.is_empty());
    }

    #[test]
    fn writing_in_the_middle_overwrites_rather_than_grows() {
        let stream = MemoryStream::with_data(b"aaaaaa".to_vec());
        seek(&stream, 2, SET);
        write(&stream, b"BB");
        assert_eq!(stream.data(), b"aaBBaa");
    }

    #[test]
    fn seeking_from_each_origin_lands_in_the_same_place() {
        let stream = MemoryStream::with_data(b"0123456789".to_vec());

        assert_eq!(seek(&stream, 4, SET), (kResultOk, 4));
        assert_eq!(seek(&stream, 2, CUR), (kResultOk, 6));
        assert_eq!(seek(&stream, -2, END), (kResultOk, 8));

        let (_, bytes) = read(&stream, 2);
        assert_eq!(bytes, b"89");
    }

    #[test]
    fn seeking_outside_the_data_is_clamped_to_it() {
        let stream = MemoryStream::with_data(b"0123".to_vec());
        assert_eq!(seek(&stream, 100, SET), (kResultOk, 4));
        assert_eq!(seek(&stream, -100, SET), (kResultOk, 0));
    }

    #[test]
    fn an_unknown_seek_mode_is_refused() {
        let stream = MemoryStream::with_data(b"0123".to_vec());
        let (result, _) = seek(&stream, 0, 99);
        assert_eq!(result, kInvalidArgument);
    }

    #[test]
    fn tell_follows_reads_and_writes() {
        let stream = MemoryStream::new();
        let mut at: int64 = -1;
        assert_eq!(unsafe { stream.tell(&mut at) }, kResultOk);
        assert_eq!(at, 0);

        write(&stream, b"1234");
        unsafe { stream.tell(&mut at) };
        assert_eq!(at, 4);

        stream.rewind();
        read(&stream, 2);
        unsafe { stream.tell(&mut at) };
        assert_eq!(at, 2);
    }

    #[test]
    fn a_null_buffer_is_refused_rather_than_dereferenced() {
        let stream = MemoryStream::new();
        let mut n: int32 = 0;
        assert_eq!(
            unsafe { stream.read(std::ptr::null_mut(), 4, &mut n) },
            kInvalidArgument
        );
        assert_eq!(
            unsafe { stream.write(std::ptr::null_mut(), 4, &mut n) },
            kInvalidArgument
        );
    }

    #[test]
    fn a_negative_length_is_refused() {
        let stream = MemoryStream::new();
        let mut byte = 0u8;
        let mut n: int32 = 0;
        assert_eq!(
            unsafe { stream.read(&mut byte as *mut u8 as *mut c_void, -1, &mut n) },
            kInvalidArgument
        );
        assert_eq!(
            unsafe { stream.write(&mut byte as *mut u8 as *mut c_void, -1, &mut n) },
            kInvalidArgument
        );
    }

    #[test]
    fn the_counts_may_be_null() {
        let stream = MemoryStream::new();
        let bytes = b"x";
        assert_eq!(
            unsafe {
                stream.write(
                    bytes.as_ptr() as *mut c_void,
                    1,
                    std::ptr::null_mut(),
                )
            },
            kResultOk
        );
        stream.rewind();
        let mut buffer = [0u8; 1];
        assert_eq!(
            unsafe {
                stream.read(
                    buffer.as_mut_ptr() as *mut c_void,
                    1,
                    std::ptr::null_mut(),
                )
            },
            kResultOk
        );
        assert_eq!(&buffer, b"x");
    }
}
