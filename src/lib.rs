#![allow(dead_code)]
//! # Scroll
//!
//! ```text, no_run
//!         _______________
//!    ()==(              (@==()
//!         '______________'|
//!           |             |
//!           |   ἀρετή     |
//!         __)_____________|
//!    ()==(               (@==()
//!         '--------------'
//!
//! ```
//!
//! Scroll implements several traits for read/writing generic containers (byte buffers are currently implemented by default). Most familiar will likely be the `Pread` trait, which at its basic takes an immutable reference to self, an immutable offset to read at, (and a parsing context, more on that later), and then returns the deserialized value.
//!
//! Because self is immutable, _**all** reads can be performed in parallel_ and hence are trivially parallelizable.
//!
//! A simple example demonstrates its flexibility:
//!
//! ```rust
//! use scroll::Pread;
//! let bytes: [u8; 4] = [0xde, 0xad, 0xbe, 0xef];
//!
//! // we can use the Buffer type that scroll provides, or use it on regular byte slices (or anything that impl's `AsRef<[u8]>`)
//! //let buffer = scroll::Buffer::new(bytes);
//! let b = &bytes[..];
//!
//! // reads a u32 out of `b` with Big Endian byte order, at offset 0
//! let i: u32 = b.pread(0, scroll::BE).unwrap();
//! // or a u16 - specify the type either on the variable or with the beloved turbofish
//! let i2 = b.pread::<u16>(2, scroll::BE).unwrap();
//!
//! // We can also skip the ctx by calling `pread_into`.
//! // for the primitive numbers, this will default to the host machine endianness (technically it is whatever default `Ctx` the target type is impl'd for)
//! let byte: u8 = b.pread_into(0).unwrap();
//! let i3: u32 = b.pread_into(0).unwrap();
//!
//! // this will have the type `scroll::Error::BadOffset` because it tried to read beyond the bound
//! let byte: scroll::Result<i64> = b.pread_into(0);
//!
//! // we can also get str and byte references from the underlying buffer/bytes using `pread_slice`
//! let slice = b.pread_slice::<str>(0, 2).unwrap();
//! let byte_slice: &[u8] = b.pread_slice(0, 2).unwrap();
//!
//! // finally, we can also parse out custom datatypes if they implement the conversion trait `TryFromCtx`
//! let leb128_bytes: [u8; 5] = [0xde | 128, 0xad | 128, 0xbe | 128, 0xef | 128, 0x1];
//! // parses a uleb128 (variable length encoded integer) from the above bytes
//! let uleb128: u64 = leb128_bytes.pread::<scroll::Uleb128>(0, scroll::LEB128).unwrap().into();
//! assert_eq!(uleb128, 0x01def96deu64);
//! ```
//!
//! # Advanced Uses
//!
//! Scroll is designed to be highly configurable - it allows you to implement various context (`Ctx`) sensitive traits, which then grants the implementor _automatic_ uses of the `Pread`/`Gread` and/or `Pwrite`/`Gwrite` traits.
//!
//! For example, suppose we have a datatype and we want to specify how to parse or serialize this datatype out of some arbitrary
//! byte buffer. In order to do this, we need to provide a `TryFromCtx` impl for our datatype.
//! 
//! In particular, if we do this for the `[u8]` target, using the convention `(usize, YourCtx)`, you will automatically get access to
//! calling `pread::<YourDatatype>` on arrays of bytes.
//! 
//! ```rust
//! use scroll::{self, ctx, Pread, BE};
//! 
//! struct Data<'a> {
//!   name: &'a str,
//!   id: u32,
//! }
//! 
//! // we could use a `(usize, endian::Scroll)` if we wanted
//! #[derive(Debug, Clone, Copy, Default)]
//! struct DataCtx { pub size: usize, pub endian: scroll::Endian }
//! 
//! // note the lifetime specified here
//! impl<'a> ctx::TryFromCtx<'a, (usize, DataCtx)> for Data<'a> {
//!   type Error = scroll::Error;
//!   // and the lifetime annotation on `&'a [u8]` here
//!   fn try_from_ctx (src: &'a [u8], (offset, DataCtx {size, endian}): (usize, DataCtx))
//!     -> Result<Self, Self::Error> {
//!     let name = src.pread_slice::<str>(offset, size)?;
//!     let id = src.pread(offset+size, endian)?;
//!     Ok(Data { name: name, id: id })
//!   }
//! }
//! 
//! let bytes = scroll::Buffer::new(b"UserName\x01\x02\x03\x04");
//! let data = bytes.pread::<Data>(0, DataCtx { size: 8, endian: BE }).unwrap();
//! assert_eq!(data.id, 0x01020304);
//! assert_eq!(data.name.to_string(), "UserName".to_string());
//! ```
//!
//! Please see the [Pread documentation examples](trait.Pread.html#implementing-your-own-reader)

#![cfg_attr(not(feature = "std"), no_std)]

#[cfg(feature = "std")]
extern crate core;

pub mod ctx;
mod measure;
mod pread;
mod pwrite;
mod greater;
mod buffer;
mod error;
mod endian;
mod leb128;
#[cfg(feature = "std")]
mod lesser;

pub use measure::Measure;
pub use endian::*;
pub use pread::*;
pub use pwrite::*;
pub use greater::*;
pub use buffer::*;
pub use error::*;
pub use leb128::*;
#[cfg(feature = "std")]
pub use lesser::*;

#[cfg(test)]
mod tests {
    #[allow(overflowing_literals)]
    use super::{LE, Buffer};

    // cursor needs to implement AsRef<[u8]>
    // #[test]
    // fn test_measurable_on_cursor() {
    //     use std::io::Cursor;
    //     use super::Measure;
    //     let bytes: [u8; 4] = [0xef, 0xbe, 0xad, 0xde];
    //     let cursor = Cursor::new(bytes);
    //     assert_eq!(cursor.measure(), 4);
    // }

    #[test]
    fn test_measurable() {
        use super::Measure;
        let bytes: [u8; 4] = [0xef, 0xbe, 0xad, 0xde];
        assert_eq!(bytes.measure(), 4);
    }

    //////////////////////////////////////////////////////////////
    // begin pread
    //////////////////////////////////////////////////////////////

    macro_rules! pwrite_test {
        ($write:ident, $read:ident, $deadbeef:expr) => {
            #[test]
            fn $write() {
                use super::{Pwrite, Pread, BE};
                let bytes: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
                let mut b = Buffer::new(&bytes[..]);
                b.pwrite::<$read>($deadbeef, 0, LE).unwrap();
                assert_eq!(b.pread::<$read>(0, LE).unwrap(), $deadbeef);
                b.pwrite::<$read>($deadbeef, 0, BE).unwrap();
                assert_eq!(b.pread::<$read>(0, BE).unwrap(), $deadbeef);
            }
        }
    }

    pwrite_test!(p_u16, u16, 0xbeef);
    pwrite_test!(p_i16, i16, 0x7eef);
    pwrite_test!(p_u32, u32, 0xbeefbeef);
    pwrite_test!(p_i32, i32, 0x7eefbeef);
    pwrite_test!(p_u64, u64, 0xbeefbeef7eef7eef);
    pwrite_test!(p_i64, i64, 0x7eefbeef7eef7eef);

    #[test]
    fn pread_be() {
        use super::{Pread};
        let bytes: [u8; 2] = [0x7e, 0xef];
        let b = &bytes[..];
        let byte: u16 = <[u8] as Pread>::pread(b, 0, super::BE).unwrap();
        assert_eq!(0x7eef, byte);
        let bytes: [u8; 2] = [0xde, 0xad];
        let dead: u16 = bytes.pread(0, super::BE).unwrap();
        assert_eq!(0xdead, dead);
    }

    #[test]
    fn pread_into() {
        use super::{Pread};
        let bytes: [u8; 2] = [0x7e, 0xef];
        let b = &bytes[..];
        let byte: u16 = b.pread_into(0).unwrap();
        assert_eq!(0xef7e, byte);
    }

    #[test]
    fn pread_slice() {
        use super::{Pread};
        let bytes: [u8; 2] = [0x7e, 0xef];
        let b = &bytes[..];
        let _bytes2: Result<&str, _>  = b.pread_slice::<str>(0, 2);
        let bytes2: &[u8]  = b.pread_slice(0, 2).unwrap();
        //let bytes3: &[u8; 2]  = b.pread_slice(0, 2).unwrap();
        assert_eq!(bytes2.len(), bytes[..].len());
        for i in 0..bytes2.len() {
            assert_eq!(bytes2[i], bytes[i])
        }
    }

    use std::error;
    use std::fmt::{self, Display};

    #[derive(Debug)]
    pub struct ExternalError {}

    impl Display for ExternalError {
        fn fmt(&self, fmt: &mut fmt::Formatter) -> fmt::Result {
            write!(fmt, "ExternalError")
        }
    }

    impl error::Error for ExternalError {
        fn description(&self) -> &str {
            "ExternalError"
        }
        fn cause(&self) -> Option<&error::Error> { None}
    }

    impl From<super::Error> for ExternalError {
        fn from(err: super::Error) -> Self {
            //use super::Error::*;
            match err {
                _ => ExternalError{},
            }
        }
    }

    #[derive(Debug, PartialEq, Eq)]
    pub struct Foo(u16);

    impl super::ctx::TryIntoCtx for Foo {
        type Error = ExternalError;
        fn try_into_ctx(self, this: &mut [u8], ctx: (usize, super::Endian)) -> Result<(), Self::Error> {
            use super::Pwrite;
            let offset = ctx.0;
            let le = ctx.1;
            if offset > 2 { return Err((ExternalError {}).into()) }
            this.pwrite(self.0, offset, le)?;
            Ok(())
        }
    }

    impl<'a> super::ctx::TryFromCtx<'a> for Foo {
        type Error = ExternalError;
        fn try_from_ctx(this: &'a [u8], ctx: (usize, super::Endian)) -> Result<Self, Self::Error> {
            use super::Pread;
            let offset = ctx.0;
            let le = ctx.1;
            if offset > 2 { return Err((ExternalError {}).into()) }
            let n = this.pread(offset, le)?;
            Ok(Foo(n))
        }
    }

    #[test]
    /// This ensures the raw byte reading api works
    fn p_bytes_api () {
        use super::{Pread, Pwrite, LE, BE};
        let mut bytes: [u8; 4] = [0xde, 0xaf, 0, 0];
        {
            let b = &bytes[..];
            let res = b.pread::<u16>(0, LE).unwrap();
            assert_eq!(0xafde, res);
            assert_eq!(0xdeaf, b.pread::<u16>(0, BE).unwrap());
            fn _pread_api<S: super::Pread>(bytes: &S) -> Result<u16, super::Error> {
                bytes.pread(0, super::LE)
            }
            let res = _pread_api(&b).unwrap();
            assert_eq!(res, 0xafde);
            fn _pread_api_external<S: super::Pread<ExternalError>>(bytes: &S) -> Result<Foo, ExternalError> {
                let b = [0, 0];
                let b = &b[..];
                let _: u16 = b.pread_into(0)?;
                bytes.pread(0, super::LE)
            }
            let foo = _pread_api_external(&b).unwrap();
            assert_eq!(Foo(45022), foo);
        }
        {
            let mut b = &mut bytes[..];
            let () = b.pwrite::<u16>(0xdeadu16, 2, BE).unwrap();
            assert_eq!(0xdead, b.pread::<u16>(2, BE).unwrap());
            fn _pwrite_api<S: ?Sized + super::Pwrite<ExternalError>>(bytes: &mut S) -> Result<(), ExternalError> {
                bytes.pwrite(Foo(0x7f), 1, super::LE)
            }
            let ()  = _pwrite_api(b).unwrap();
            assert_eq!(b[1], 0x7f);
        }
    }

    #[test]
    /// This ensures the buffer reading api works
    fn p_buffer_api() {
        let bytes: [u8; 4] = [0, 0, 0xde | 128, 1];
        let mut b = Buffer::new(&bytes[..]);
        //let mut b = &bytes[..];
        // parses using multiple pread contexts
        fn _pread_api<S: super::Pread + super::Pread<super::Error, super::Leb128>>(bytes: &S) -> Result<u16, super::Error> {
            let _res: u32 = bytes.pread_into(0)?;
            let _slice: &[u8] = bytes.pread_slice(0, 4)?;
            let _unwrapped: u8 = bytes.pread_unsafe(0, super::LE);
            let _uleb: super::Uleb128 = bytes.pread(2, super::LEB128).unwrap();
            bytes.pread(0, super::LE)
        }
        fn _pwrite_api<S: super::Pwrite>(bytes: &mut S) -> Result<(), super::Error> {
            bytes.pwrite(42u8, 0, super::LE)
        }
//        fn _pwrite_api2<S: super::Pwrite + super::Pwrite<ExternalError>>(bytes: &mut S) -> Result<(), super::Error<ExternalError>> {
        fn _pwrite_api2<S: super::Pwrite<ExternalError>>(bytes: &mut S) -> Result<(), ExternalError> {
            
            bytes.pwrite(Foo(0x7f), 1, super::LE)
        }
        let ()  = _pwrite_api(&mut b).unwrap();
        assert_eq!(b[0], 42);
        let res = _pread_api(&b).unwrap();
        assert_eq!(res, 42);
        let ()  = _pwrite_api2(&mut b).unwrap();
        let res = <[u8] as super::Pread>::pread_into::<u8>(&b, 1).unwrap();
        assert_eq!(res, 0x7f);
        let res = <[u8] as super::Pwrite<ExternalError>>::pwrite(&mut b, Foo(0x7f), 3, super::LE);
        assert!(res.is_err());
    }

    #[test]
    fn pread_iter_bytes() {
        use super::{Pread};
        let mut bytes_to: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
        let bytes_from: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut bytes_to = &mut bytes_to[..];
        let bytes_from = &bytes_from[..];
        for i in 0..bytes_from.len() {
            bytes_to[i] = bytes_from.pread_into(i).unwrap();
        }
        assert_eq!(bytes_to, bytes_from);
    }

    //////////////////////////////////////////////////////////////
    // end pread
    //////////////////////////////////////////////////////////////

    //////////////////////////////////////////////////////////////
    // begin gread
    //////////////////////////////////////////////////////////////

    macro_rules! simple_gread_test {
        ($read:ident, $deadbeef:expr, $typ:ty) => {
            #[test]
            fn $read() {
                use super::Gread;
                let bytes: [u8; 8] = [0xf, 0xe, 0xe, 0xb, 0xd, 0xa, 0xe, 0xd];
                let buffer = Buffer::new(bytes);
                let mut offset = 0;
                let deadbeef: $typ = buffer.gread(&mut offset, LE).unwrap();
                assert_eq!(deadbeef, $deadbeef as $typ);
                assert_eq!(offset, ::std::mem::size_of::<$typ>());
            }
        }
    }

    simple_gread_test!(simple_gread_f32, 0xb0e0e0f, f32);
    simple_gread_test!(simple_gread_u16, 0xe0f, u16);
    simple_gread_test!(simple_gread_u32, 0xb0e0e0f, u32);
    simple_gread_test!(simple_gread_u64, 0xd0e0a0d0b0e0e0f, u64);
    simple_gread_test!(simple_gread_i64, 940700423303335439, i64);
    simple_gread_test!(simple_gread_f64, 0xd0e0a0d0b0e0e0fu64, f64);

    // useful for ferreting out problems with impls
    #[test]
    fn gread_iter_bytes() {
        use super::{Gread};
        let mut bytes_to: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
        let bytes_from: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let mut bytes_to = &mut bytes_to[..];
        let bytes_from = &bytes_from[..];
        let mut offset = &mut 0;
        for i in 0..bytes_from.len() {
            bytes_to[i] = bytes_from.gread_into(&mut offset).unwrap();
        }
        assert_eq!(bytes_to, bytes_from);
        assert_eq!(*offset, bytes_to.len());
    }

    #[test]
    fn gread_inout() {
        use super::{Gread};
        let mut bytes_to: [u8; 8] = [0, 0, 0, 0, 0, 0, 0, 0];
        let bytes_from: [u8; 8] = [1, 2, 3, 4, 5, 6, 7, 8];
        let bytes = &bytes_from[..];
        let mut offset = &mut 0;
        bytes.gread_inout(offset, &mut bytes_to[..]).unwrap();
        assert_eq!(bytes_to, bytes_from);
        assert_eq!(*offset, bytes_to.len());
    }

    #[test]
    fn gread_byte() {
        use super::{Gread};
        let bytes: [u8; 1] = [0x7f];
        let b = Buffer::new(&bytes[..]);
        let mut offset = &mut 0;
        let byte: u8 = b.gread_into(offset).unwrap();
        assert_eq!(0x7f, byte);
        assert_eq!(*offset, 1);
    }

    #[test]
    fn gread_slice() {
        use super::{Gread};
        let bytes: [u8; 2] = [0x7e, 0xef];
        let b = &bytes[..];
        let mut offset = &mut 0;
        let res = b.gread_slice::<str>(offset, 3);
        assert!(res.is_err());
        *offset = 0;
        let astring: [u8; 3] = [0x45, 042, 0x44];
        let string: &str = astring.gread_slice(offset, 2).unwrap();
        assert_eq!(string, "E*");
        *offset = 0;
        let bytes2: &[u8]  = b.gread_slice(offset, 2).unwrap();
        assert_eq!(*offset, 2);
        assert_eq!(bytes2.len(), bytes[..].len());
        for i in 0..bytes2.len() {
            assert_eq!(bytes2[i], bytes[i])
        }
    }

    #[test]
    /// This ensures the raw byte g-reading api works
    fn g_bytes_api () {
        use super::{Gread, LE, BE};
        let bytes: [u8; 4] = [0xde, 0xaf, 0, 0];
        {
            let b = &bytes[..];
            let res = b.gread::<u16>(&mut 0, LE).unwrap();
            assert_eq!(0xafde, res);
            assert_eq!(0xdeaf, b.gread::<u16>(&mut 0, BE).unwrap());
            fn _gread_api<S: super::Gread>(bytes: &S) -> Result<u16, super::Error> {
                // we just check if these actually work inside a generic parameter
                let _res: u32 = bytes.gread_into(&mut 0)?;
                let _slice: &[u8] = bytes.gread_slice(&mut 0, 4)?;
                let _unwrapped: u8 = bytes.gread_unsafe(&mut 0, super::LE);
                bytes.gread(&mut 0, super::LE)
            }
            let res = _gread_api(&b).unwrap();
            assert_eq!(res, 0xafde);
            // fn _gread_api_external<S: super::Gread + super::Gread<ExternalError>>(bytes: &S) -> Result<Foo, ExternalError> {
            //     let b = [0, 0];
            //     let b = &b[..];
            //     let _: u16 = b.gread_into(&mut 0)?;
            //     bytes.gread(&mut 0, super::LE)?
            // }
            // let foo = _gread_api_external(&b).unwrap();
            // assert_eq!(Foo(45022), foo);
        }
    }

    /////////////////////////////////////////////////////////////////
    // end gread
    /////////////////////////////////////////////////////////////////
}
