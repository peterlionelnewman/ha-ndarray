#[cfg(feature = "opencl")]
extern crate ocl;

use std::convert::identity;
use std::fmt;
use std::iter::Sum;
use std::ops::{Add, Div, Mul, Range, Rem, Sub};
use std::sync::{Arc, RwLock};

use rayon::prelude::*;
#[allow(unused_imports)]
use safecast::{as_type, AsType};

pub use array::*;
use ops::*;

mod array;
#[cfg(feature = "opencl")]
mod cl_programs;
pub mod ops;

pub mod construct {
    pub use super::ops::{RandomNormal, RandomUniform};
}

const GPU_MIN_DEFAULT: usize = 1024;

pub enum Error {
    Bounds(String),
    Interface(String),
    #[cfg(feature = "opencl")]
    OCL(ocl::Error),
}

#[cfg(feature = "opencl")]
impl From<ocl::Error> for Error {
    fn from(cause: ocl::Error) -> Self {
        Self::OCL(cause)
    }
}

impl fmt::Debug for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Bounds(cause) => f.write_str(cause),
            Self::Interface(cause) => f.write_str(cause),
            #[cfg(feature = "opencl")]
            Self::OCL(cause) => cause.fmt(f),
        }
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Bounds(cause) => f.write_str(cause),
            Self::Interface(cause) => f.write_str(cause),
            #[cfg(feature = "opencl")]
            Self::OCL(cause) => cause.fmt(f),
        }
    }
}

impl std::error::Error for Error {}

pub type Shape = Vec<usize>;

// TODO: is there a better way to implement the OclPrm trait bound?
#[cfg(feature = "opencl")]
pub trait CDatatype:
    ocl::OclPrm
    + Copy
    + Add<Output = Self>
    + Div<Output = Self>
    + Mul<Output = Self>
    + Rem<Output = Self>
    + Sub<Output = Self>
    + PartialEq
    + PartialOrd
    + Sum
    + Send
    + Sync
    + 'static
{
    const TYPE_STR: &'static str;

    type Float: Float;
    type Neg: CDatatype;

    fn max() -> Self;

    fn min() -> Self;

    fn one() -> Self;

    fn zero() -> Self;

    fn from_f64(float: f64) -> Self;

    fn abs(self) -> Self;

    fn exp(self) -> Self;

    fn log(self, base: f64) -> Self {
        Self::from_f64(self.to_f64().log(base))
    }

    fn neg(self) -> Self::Neg;

    fn not(self) -> u8 {
        if self == Self::zero() {
            1
        } else {
            0
        }
    }

    fn pow(self, exp: f64) -> Self {
        Self::from_f64(self.to_f64().powf(exp))
    }

    fn to_f64(self) -> f64;
}

#[cfg(not(feature = "opencl"))]
pub trait CDatatype:
    Copy
    + Add<Output = Self>
    + Div<Output = Self>
    + Mul<Output = Self>
    + Rem<Output = Self>
    + Sub<Output = Self>
    + PartialEq
    + PartialOrd
    + Sum
    + Send
    + Sync
    + 'static
{
    const TYPE_STR: &'static str;

    type Float: Float;
    type Neg: CDatatype;

    fn max() -> Self;

    fn min() -> Self;

    fn one() -> Self;

    fn zero() -> Self;

    fn from_f64(float: f64) -> Self;

    fn abs(self) -> Self;

    fn exp(self) -> Self;

    fn log(self, base: f64) -> Self {
        Self::from_f64(self.to_f64().log(base))
    }

    fn neg(self) -> Self::Neg;

    fn not(self) -> u8 {
        if self == Self::zero() {
            1
        } else {
            0
        }
    }

    fn pow(self, exp: f64) -> Self {
        Self::from_f64(self.to_f64().powf(exp))
    }

    fn to_f64(self) -> f64;
}

macro_rules! c_type {
    ($t:ty, $ct:expr, $max:expr, $min: expr, $one:expr, $zero:expr, $abs:expr, $float:ty, $neg:ty) => {
        impl CDatatype for $t {
            const TYPE_STR: &'static str = $ct;

            type Float = $float;
            type Neg = $neg;

            fn max() -> Self {
                $max
            }

            fn min() -> Self {
                $min
            }

            fn one() -> Self {
                $one
            }

            fn zero() -> Self {
                $zero
            }

            fn from_f64(float: f64) -> Self {
                float as $t
            }

            fn abs(self) -> Self {
                $abs(self)
            }

            fn exp(self) -> Self {
                Self::from_f64(std::f64::consts::E.pow(self.to_f64()))
            }

            fn neg(self) -> Self::Neg {
                if self >= Self::zero() {
                    self as $neg
                } else {
                    -(self as $neg)
                }
            }

            fn to_f64(self) -> f64 {
                self as f64
            }
        }
    };
}

c_type!(f32, "float", f32::MAX, f32::MIN, 1., 0., f32::abs, f32, f32);
c_type!(
    f64,
    "double",
    f64::MAX,
    f64::MIN,
    1.,
    0.,
    f64::abs,
    f64,
    f64
);
c_type!(u8, "uchar", u8::MAX, u8::MIN, 1, 0, identity, f32, i8);
c_type!(u16, "ushort", u16::MAX, u16::MIN, 1, 0, identity, f32, i16);
c_type!(u32, "uint", u32::MAX, u32::MIN, 1, 0, identity, f32, i32);
c_type!(u64, "ulong", u64::MAX, u64::MIN, 1, 0, identity, f64, i64);
c_type!(i8, "char", i8::MAX, i8::MIN, 1, 0, i8::abs, f32, i8);
c_type!(i16, "short", i16::MAX, i16::MIN, 1, 0, i16::abs, f32, i16);
c_type!(i32, "int", i32::MAX, i32::MIN, 1, 0, i32::abs, f32, i32);
c_type!(i64, "long", i64::MAX, i64::MIN, 1, 0, i64::abs, f64, i64);

pub trait Float: CDatatype {
    fn is_inf(self) -> u8;

    fn is_nan(self) -> u8;
}

impl Float for f32 {
    fn is_inf(self) -> u8 {
        if f32::is_infinite(self) {
            1
        } else {
            0
        }
    }

    fn is_nan(self) -> u8 {
        if f32::is_nan(self) {
            1
        } else {
            0
        }
    }
}

impl Float for f64 {
    fn is_inf(self) -> u8 {
        if f64::is_infinite(self) {
            1
        } else {
            0
        }
    }

    fn is_nan(self) -> u8 {
        if f64::is_nan(self) {
            1
        } else {
            0
        }
    }
}

#[cfg(feature = "opencl")]
#[derive(Clone, Default)]
struct DeviceList {
    devices: Vec<ocl::Device>,
    next: std::sync::Arc<std::sync::atomic::AtomicUsize>,
}

#[cfg(feature = "opencl")]
impl DeviceList {
    fn is_empty(&self) -> bool {
        self.devices.is_empty()
    }

    fn next(&self) -> Option<ocl::Device> {
        if self.devices.is_empty() {
            None
        } else {
            let idx = self.next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            self.devices.get(idx % self.devices.len()).copied()
        }
    }
}

#[cfg(feature = "opencl")]
impl From<Vec<ocl::Device>> for DeviceList {
    fn from(devices: Vec<ocl::Device>) -> Self {
        Self {
            devices,
            next: std::sync::Arc::new(Default::default()),
        }
    }
}

#[cfg(feature = "opencl")]
impl FromIterator<ocl::Device> for DeviceList {
    fn from_iter<T: IntoIterator<Item = ocl::Device>>(iter: T) -> Self {
        Self::from(iter.into_iter().collect::<Vec<ocl::Device>>())
    }
}

pub trait BufferInstance: Clone + Send + Sync {
    type DType: CDatatype;

    fn size(&self) -> usize;
}

pub trait BufferReduce {
    type DType: CDatatype;

    fn all(&self, queue: &Queue) -> Result<bool, Error>;

    fn any(&self, queue: &Queue) -> Result<bool, Error>;

    fn max(&self, queue: &Queue) -> Result<Self::DType, Error>;

    fn min(&self, queue: &Queue) -> Result<Self::DType, Error>;

    fn product(&self, queue: &Queue) -> Result<Self::DType, Error>;

    fn sum(&self, queue: &Queue) -> Result<Self::DType, Error>;
}

#[derive(Clone)]
pub enum SliceConverter<'a, T> {
    Vec(Vec<T>),
    Slice(&'a [T]),
}

impl<'a, T: Clone> SliceConverter<'a, T> {
    pub fn into_vec(self) -> Vec<T> {
        match self {
            Self::Vec(vec) => vec,
            Self::Slice(slice) => slice.to_vec(),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            Self::Vec(vec) => vec.len(),
            Self::Slice(slice) => slice.len(),
        }
    }
}

impl<'a, T> AsRef<[T]> for SliceConverter<'a, T> {
    fn as_ref(&self) -> &[T] {
        match self {
            Self::Vec(data) => data.as_slice(),
            Self::Slice(slice) => slice,
        }
    }
}

#[cfg(feature = "opencl")]
#[derive(Clone)]
pub enum CLConverter<'a, T: CDatatype> {
    Owned(ocl::Buffer<T>),
    Borrowed(&'a ocl::Buffer<T>),
}

#[cfg(feature = "opencl")]
impl<'a, T: CDatatype> CLConverter<'a, T> {
    pub fn into_buffer(self) -> Result<ocl::Buffer<T>, Error> {
        match self {
            Self::Owned(buffer) => Ok(buffer),
            Self::Borrowed(buffer) => {
                let cl_queue = buffer.default_queue().expect("OpenCL queue");
                let mut copy = ocl::Buffer::builder()
                    .queue(cl_queue.clone())
                    .len(buffer.len())
                    .build()?;

                buffer.copy(&mut copy, None, None).enq()?;

                Ok(copy)
            }
        }
    }

    pub fn len(&self) -> usize {
        match self {
            Self::Owned(buffer) => buffer.len(),
            Self::Borrowed(buffer) => buffer.len(),
        }
    }
}

#[cfg(feature = "opencl")]
impl<'a, T: CDatatype> AsRef<ocl::Buffer<T>> for CLConverter<'a, T> {
    fn as_ref(&self) -> &ocl::Buffer<T> {
        match self {
            Self::Owned(buffer) => &buffer,
            Self::Borrowed(buffer) => buffer,
        }
    }
}

#[derive(Clone)]
pub enum BufferConverter<'a, T: CDatatype> {
    Host(SliceConverter<'a, T>),
    #[cfg(feature = "opencl")]
    CL(CLConverter<'a, T>),
}

impl<'a, T: CDatatype> BufferConverter<'a, T> {
    pub fn into_buffer(self) -> Result<Buffer<T>, Error> {
        match self {
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => buffer.into_buffer().map(Buffer::CL),
            Self::Host(buffer) => Ok(Buffer::Host(buffer.into_vec())),
        }
    }

    pub fn size(&self) -> usize {
        match self {
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => buffer.len(),
            Self::Host(buffer) => buffer.size(),
        }
    }

    #[cfg(feature = "opencl")]
    pub fn to_cl(self, queue: &Queue) -> Result<CLConverter<'a, T>, Error> {
        match self {
            Self::CL(buffer) => Ok(buffer),
            Self::Host(buffer) => {
                let buffer = buffer.as_ref();

                let cl_queue = if let Some(cl_queue) = &queue.cl_queue {
                    cl_queue.clone()
                } else {
                    let cl_context = queue.context().cl_context();
                    let size_hint = Ord::max(buffer.len(), queue.context.gpu_min);
                    let device = queue
                        .context()
                        .select_device(size_hint)
                        .expect("OpenCL device");

                    ocl::Queue::new(cl_context, device, None)?
                };

                let buffer = ocl::Buffer::builder()
                    .queue(cl_queue)
                    .len(buffer.len())
                    .copy_host_slice(buffer)
                    .build()?;

                Ok(CLConverter::Owned(buffer))
            }
        }
    }

    pub fn to_slice(self) -> Result<SliceConverter<'a, T>, Error> {
        match self {
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => {
                let buffer = buffer.as_ref();
                let mut copy = vec![T::default(); buffer.len()];
                buffer.read(&mut copy[..]).enq()?;
                Ok(SliceConverter::Vec(copy))
            }
            Self::Host(buffer) => Ok(buffer),
        }
    }
}

macro_rules! buffer_reduce {
    ($this:ident, $var:ident, $call:expr) => {
        match $this {
            Self::Host(buffer) => match buffer {
                SliceConverter::Vec($var) => $call,
                SliceConverter::Slice($var) => $call,
            },
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => match buffer {
                CLConverter::Owned($var) => $call,
                CLConverter::Borrowed($var) => $call,
            },
        }
    };
}

impl<'a, T: CDatatype> BufferReduce for BufferConverter<'a, T> {
    type DType = T;

    fn all(&self, queue: &Queue) -> Result<bool, Error> {
        buffer_reduce!(self, this, this.all(queue))
    }

    fn any(&self, queue: &Queue) -> Result<bool, Error> {
        buffer_reduce!(self, this, this.any(queue))
    }

    fn max(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_reduce!(self, this, this.max(queue))
    }

    fn min(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_reduce!(self, this, this.min(queue))
    }

    fn product(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_reduce!(self, this, this.product(queue))
    }

    fn sum(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_reduce!(self, this, this.sum(queue))
    }
}

impl<'a, T: CDatatype> From<Vec<T>> for BufferConverter<'a, T> {
    fn from(buffer: Vec<T>) -> Self {
        Self::Host(SliceConverter::Vec(buffer))
    }
}

impl<'a, T: CDatatype> From<&'a [T]> for BufferConverter<'a, T> {
    fn from(buffer: &'a [T]) -> Self {
        Self::Host(SliceConverter::Slice(buffer))
    }
}

#[cfg(feature = "opencl")]
impl<'a, T: CDatatype> From<ocl::Buffer<T>> for BufferConverter<'a, T> {
    fn from(buffer: ocl::Buffer<T>) -> Self {
        Self::CL(CLConverter::Owned(buffer))
    }
}

#[cfg(feature = "opencl")]
impl<'a, T: CDatatype> From<&'a ocl::Buffer<T>> for BufferConverter<'a, T> {
    fn from(buffer: &'a ocl::Buffer<T>) -> Self {
        Self::CL(CLConverter::Borrowed(buffer))
    }
}

impl<'a, T: CDatatype> From<Buffer<T>> for BufferConverter<'a, T> {
    fn from(buffer: Buffer<T>) -> Self {
        match buffer {
            Buffer::Host(buffer) => BufferConverter::Host(SliceConverter::Vec(buffer)),
            #[cfg(feature = "opencl")]
            Buffer::CL(buffer) => BufferConverter::CL(CLConverter::Owned(buffer)),
        }
    }
}

impl<'a, T: CDatatype> From<&'a Buffer<T>> for BufferConverter<'a, T> {
    fn from(buffer: &'a Buffer<T>) -> Self {
        match buffer {
            Buffer::Host(buffer) => BufferConverter::Host(SliceConverter::Slice(buffer)),
            #[cfg(feature = "opencl")]
            Buffer::CL(buffer) => BufferConverter::CL(CLConverter::Borrowed(buffer)),
        }
    }
}

impl<T: CDatatype> BufferInstance for Vec<T> {
    type DType = T;

    fn size(&self) -> usize {
        self.len()
    }
}

impl<T: CDatatype> BufferReduce for Vec<T> {
    type DType = T;

    fn all(&self, _queue: &Queue) -> Result<bool, Error> {
        let zero = Self::DType::zero();
        Ok(self.par_iter().copied().all(|n| n != zero))
    }

    fn any(&self, _queue: &Queue) -> Result<bool, Error> {
        let zero = Self::DType::zero();
        Ok(self.par_iter().copied().any(|n| n != zero))
    }

    fn max(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r > l {
                r
            } else {
                l
            }
        };

        Ok(self.par_iter().copied().reduce(T::min, collector))
    }

    fn min(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r < l {
                r
            } else {
                l
            }
        };

        Ok(self.par_iter().copied().reduce(T::max, collector))
    }

    fn product(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        Ok(self.par_iter().copied().reduce(T::one, Mul::mul))
    }

    fn sum(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        Ok(self.par_iter().copied().reduce(T::zero, Add::add))
    }
}

impl<T: CDatatype> BufferReduce for [T] {
    type DType = T;

    fn all(&self, _queue: &Queue) -> Result<bool, Error> {
        let zero = Self::DType::zero();
        Ok(self.par_iter().copied().all(|n| n != zero))
    }

    fn any(&self, _queue: &Queue) -> Result<bool, Error> {
        let zero = Self::DType::zero();
        Ok(self.par_iter().copied().any(|n| n != zero))
    }

    fn max(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r > l {
                r
            } else {
                l
            }
        };

        Ok(self.par_iter().copied().reduce(T::min, collector))
    }

    fn min(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r < l {
                r
            } else {
                l
            }
        };

        Ok(self.par_iter().copied().reduce(T::max, collector))
    }

    fn product(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        Ok(self.par_iter().copied().reduce(T::one, Mul::mul))
    }

    fn sum(&self, _queue: &Queue) -> Result<Self::DType, Error> {
        Ok(self.par_iter().copied().reduce(T::zero, Add::add))
    }
}

impl<T: CDatatype> BufferInstance for Arc<Vec<T>> {
    type DType = T;

    fn size(&self) -> usize {
        self.len()
    }
}

impl<T: CDatatype> BufferInstance for Arc<RwLock<Vec<T>>> {
    type DType = T;

    fn size(&self) -> usize {
        let data = RwLock::read(self).expect("read buffer");
        data.len()
    }
}

#[cfg(feature = "opencl")]
impl<T: CDatatype> BufferInstance for ocl::Buffer<T> {
    type DType = T;

    fn size(&self) -> usize {
        self.len()
    }
}

#[cfg(feature = "opencl")]
impl<T: CDatatype> BufferReduce for ocl::Buffer<T> {
    type DType = T;

    fn all(&self, queue: &Queue) -> Result<bool, Error> {
        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce_all(cl_queue, self).map_err(Error::from)
    }

    fn any(&self, queue: &Queue) -> Result<bool, Error> {
        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce_any(cl_queue, self).map_err(Error::from)
    }

    fn max(&self, queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r > l {
                r
            } else {
                l
            }
        };

        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce(T::min(), "max", cl_queue, self, collector).map_err(Error::from)
    }

    fn min(&self, queue: &Queue) -> Result<Self::DType, Error> {
        let collector = |l, r| {
            if r < l {
                r
            } else {
                l
            }
        };

        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce(T::max(), "min", cl_queue, self, collector).map_err(Error::from)
    }

    fn product(&self, queue: &Queue) -> Result<Self::DType, Error> {
        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce(T::one(), "mul", cl_queue, self, Mul::mul).map_err(Error::from)
    }

    fn sum(&self, queue: &Queue) -> Result<Self::DType, Error> {
        let cl_queue = queue.cl_queue(self.default_queue());
        cl_programs::reduce(T::zero(), "add", cl_queue, self, Add::add).map_err(Error::from)
    }
}

#[cfg(feature = "opencl")]
impl<T: CDatatype> BufferInstance for Arc<ocl::Buffer<T>> {
    type DType = T;

    fn size(&self) -> usize {
        self.len()
    }
}

#[cfg(feature = "opencl")]
impl<T: CDatatype> BufferInstance for Arc<RwLock<ocl::Buffer<T>>> {
    type DType = T;

    fn size(&self) -> usize {
        let data = RwLock::read(self).expect("read buffer");
        data.len()
    }
}

#[cfg(feature = "opencl")]
#[derive(Clone)]
pub enum Buffer<T: CDatatype> {
    Host(Vec<T>),
    CL(ocl::Buffer<T>),
}

#[cfg(not(feature = "opencl"))]
#[derive(Clone)]
pub enum Buffer<T: CDatatype> {
    Host(Vec<T>),
}

impl<T: CDatatype> Buffer<T> {
    pub fn len(&self) -> usize {
        match self {
            Self::Host(buffer) => buffer.len(),
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => buffer.len(),
        }
    }

    pub fn write<'a, O: Into<BufferConverter<'a, T>>>(&mut self, other: O) -> Result<(), Error> {
        let other = other.into();

        match self {
            Self::Host(this) => match other {
                BufferConverter::Host(that) => this.copy_from_slice(that.as_ref()),
                #[cfg(feature = "opencl")]
                BufferConverter::CL(that) => that.as_ref().read(&mut this[..]).enq()?,
            },
            #[cfg(feature = "opencl")]
            Self::CL(this) => match other {
                BufferConverter::Host(that) => this.write(that.as_ref()).enq()?,
                BufferConverter::CL(that) => that.as_ref().copy(this, None, None).enq()?,
            },
        }

        Ok(())
    }
}

macro_rules! buffer_dispatch {
    ($this:ident, $var:ident, $call:expr) => {
        match $this {
            Self::Host($var) => $call,
            #[cfg(feature = "opencl")]
            Self::CL($var) => $call,
        }
    };
}

impl<T: CDatatype> BufferInstance for Buffer<T> {
    type DType = T;

    fn size(&self) -> usize {
        buffer_dispatch!(self, this, this.size())
    }
}

impl<T: CDatatype> BufferReduce for Buffer<T> {
    type DType = T;

    fn all(&self, queue: &Queue) -> Result<bool, Error> {
        buffer_dispatch!(self, this, BufferReduce::all(this, queue))
    }

    fn any(&self, queue: &Queue) -> Result<bool, Error> {
        buffer_dispatch!(self, this, BufferReduce::any(this, queue))
    }

    fn max(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_dispatch!(self, this, BufferReduce::max(this, queue))
    }

    fn min(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_dispatch!(self, this, BufferReduce::min(this, queue))
    }

    fn product(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_dispatch!(self, this, BufferReduce::product(this, queue))
    }

    fn sum(&self, queue: &Queue) -> Result<Self::DType, Error> {
        buffer_dispatch!(self, this, BufferReduce::sum(this, queue))
    }
}

impl<T: CDatatype> BufferInstance for Arc<Buffer<T>> {
    type DType = T;

    fn size(&self) -> usize {
        match &**self {
            Buffer::Host(buffer) => buffer.len(),
            #[cfg(feature = "opencl")]
            Buffer::CL(buffer) => buffer.len(),
        }
    }
}

impl<T: CDatatype> BufferInstance for Arc<RwLock<Buffer<T>>> {
    type DType = T;

    fn size(&self) -> usize {
        let data = RwLock::read(self).expect("read buffer");
        data.size()
    }
}

#[cfg(feature = "opencl")]
impl<T: CDatatype> From<ocl::Buffer<T>> for Buffer<T> {
    fn from(buffer: ocl::Buffer<T>) -> Self {
        Self::CL(buffer)
    }
}

impl<T: CDatatype> From<Vec<T>> for Buffer<T> {
    fn from(buffer: Vec<T>) -> Self {
        Self::Host(buffer)
    }
}

impl<T: CDatatype + fmt::Debug> fmt::Debug for Buffer<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Host(buffer) => fmt::Debug::fmt(buffer, f),
            #[cfg(feature = "opencl")]
            Self::CL(buffer) => fmt::Debug::fmt(buffer, f),
        }
    }
}

#[derive(Clone, Default)]
pub struct Platform {
    #[cfg(feature = "opencl")]
    cl_cpus: DeviceList,
    #[cfg(feature = "opencl")]
    cl_gpus: DeviceList,
    #[cfg(feature = "opencl")]
    cl_accs: DeviceList,
    #[cfg(feature = "opencl")]
    cl_platform: ocl::Platform,
}

impl Platform {
    #[cfg(feature = "opencl")]
    fn has_gpu(&self) -> bool {
        !self.cl_gpus.is_empty()
    }

    #[cfg(feature = "opencl")]
    fn next_cpu(&self) -> Option<ocl::Device> {
        self.cl_cpus.next()
    }

    #[cfg(feature = "opencl")]
    fn next_gpu(&self) -> Option<ocl::Device> {
        self.cl_gpus.next()
    }

    #[cfg(feature = "opencl")]
    fn next_acc(&self) -> Option<ocl::Device> {
        self.cl_accs.next()
    }
}

#[cfg(feature = "opencl")]
impl TryFrom<ocl::Platform> for Platform {
    type Error = ocl::Error;

    fn try_from(cl_platform: ocl::Platform) -> Result<Self, Self::Error> {
        let cl_cpus = ocl::Device::list(cl_platform, Some(ocl::DeviceType::CPU))?;
        let cl_gpus = ocl::Device::list(cl_platform, Some(ocl::DeviceType::GPU))?;
        let cl_accs = ocl::Device::list(cl_platform, Some(ocl::DeviceType::ACCELERATOR))?;

        Ok(Self {
            cl_cpus: cl_cpus.into(),
            cl_gpus: cl_gpus.into(),
            cl_accs: cl_accs.into(),
            cl_platform,
        })
    }
}

#[derive(Clone)]
#[allow(unused)]
pub struct Context {
    platform: Platform,
    gpu_min: usize,
    acc_min: usize,
    #[cfg(feature = "opencl")]
    cl_context: ocl::Context,
}

impl Context {
    #[cfg(feature = "opencl")]
    pub fn default() -> Result<Self, Error> {
        let cl_platform = ocl::Platform::first()?;
        let cl_context = ocl::Context::builder().platform(cl_platform).build()?;
        let platform = Platform::try_from(cl_platform)?;
        let acc_min = if platform.has_gpu() {
            usize::MAX
        } else {
            GPU_MIN_DEFAULT
        };

        Ok(Self {
            platform,
            gpu_min: GPU_MIN_DEFAULT,
            acc_min,
            cl_context,
        })
    }

    #[cfg(not(feature = "opencl"))]
    pub fn default() -> Result<Self, Error> {
        Ok(Self {
            platform: Platform {},
            gpu_min: GPU_MIN_DEFAULT,
            acc_min: GPU_MIN_DEFAULT,
        })
    }

    #[cfg(feature = "opencl")]
    pub fn new(gpu_min: usize, acc_min: usize, platform: Option<Platform>) -> Result<Self, Error> {
        let platform = if let Some(platform) = platform {
            platform
        } else {
            let cl_platform = ocl::Platform::first()?;
            Platform::try_from(cl_platform.clone())?
        };

        let cl_context = ocl::Context::builder()
            .platform(platform.cl_platform.clone())
            .build()?;

        Ok(Self {
            platform,
            gpu_min,
            acc_min,
            cl_context,
        })
    }

    #[cfg(not(feature = "opencl"))]
    pub fn new(gpu_min: usize, acc_min: usize, platform: Option<Platform>) -> Result<Self, Error> {
        let platform = if let Some(platform) = platform {
            platform
        } else {
            Platform::default()
        };

        Ok(Self {
            platform,
            gpu_min,
            acc_min,
        })
    }

    #[cfg(feature = "opencl")]
    fn cl_context(&self) -> &ocl::Context {
        &self.cl_context
    }

    #[cfg(feature = "opencl")]
    fn select_device(&self, size_hint: usize) -> Option<ocl::Device> {
        if size_hint < self.gpu_min {
            self.platform.next_cpu()
        } else if size_hint < self.acc_min {
            self.platform
                .next_gpu()
                .or_else(|| self.platform.next_cpu())
        } else {
            self.platform
                .next_acc()
                .or_else(|| self.platform.next_gpu())
                .or_else(|| self.platform.next_cpu())
        }
    }
}

#[derive(Clone)]
pub struct Queue {
    context: Context,
    #[cfg(feature = "opencl")]
    cl_queue: Option<ocl::Queue>,
}

impl Queue {
    fn default(context: Context) -> Self {
        Self {
            context,
            #[cfg(feature = "opencl")]
            cl_queue: None,
        }
    }

    #[cfg(feature = "opencl")]
    pub fn new(context: Context, size_hint: usize) -> Result<Self, Error> {
        if let Some(device) = context.select_device(size_hint) {
            let cl_queue = ocl::Queue::new(context.cl_context(), device, None)?;

            Ok(Self {
                context,
                cl_queue: Some(cl_queue),
            })
        } else {
            Ok(Self::default(context))
        }
    }

    #[cfg(not(feature = "opencl"))]
    pub fn new(context: Context, _size_hint: usize) -> Result<Self, Error> {
        Ok(Self::default(context))
    }

    #[cfg(feature = "opencl")]
    fn cl_queue(&self, default: Option<&ocl::Queue>) -> ocl::Queue {
        self.cl_queue
            .as_ref()
            .or_else(|| default)
            .cloned()
            .expect("OpenCL queue")
    }

    #[allow(unused)]
    fn context(&self) -> &Context {
        &self.context
    }

    #[allow(unused)]
    fn split(&self, size_hint: usize) -> Result<Self, Error> {
        #[cfg(feature = "opencl")]
        let cl_queue = if let Some(left_queue) = &self.cl_queue {
            if let Some(device) = self.context.select_device(size_hint) {
                ocl::Queue::new(&left_queue.context(), device, None).map(Some)?
            } else {
                None
            }
        } else {
            None
        };

        Ok(Self {
            context: self.context.clone(),
            #[cfg(feature = "opencl")]
            cl_queue,
        })
    }
}

pub trait NDArray: Send + Sync {
    type DType: CDatatype;

    fn context(&self) -> &Context;

    fn ndim(&self) -> usize {
        self.shape().len()
    }

    fn size(&self) -> usize {
        self.shape().iter().product()
    }

    fn shape(&self) -> &[usize];
}

impl<A: NDArray + ?Sized> NDArray for Box<A> {
    type DType = A::DType;

    fn context(&self) -> &Context {
        (**self).context()
    }

    fn shape(&self) -> &[usize] {
        (**self).shape()
    }
}

pub trait NDArrayRead: NDArray + fmt::Debug + Sized {
    fn read(&self, queue: &Queue) -> Result<BufferConverter<Self::DType>, Error>;

    fn to_host(&self, queue: &Queue) -> Result<SliceConverter<Self::DType>, Error> {
        let converter = self.read(queue)?;
        converter.to_slice()
    }

    #[cfg(feature = "opencl")]
    fn to_cl_buffer(&self, queue: &Queue) -> Result<CLConverter<Self::DType>, Error> {
        let converter = self.read(queue)?;
        converter.to_cl(queue)
    }
}

pub trait NDArrayBoolean<O>: NDArray + Sized
where
    O: NDArray<DType = Self::DType>,
{
    fn and(self, other: O) -> Result<ArrayOp<ArrayBoolean<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayBoolean::and(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn or(self, other: O) -> Result<ArrayOp<ArrayBoolean<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayBoolean::or(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn xor(self, other: O) -> Result<ArrayOp<ArrayBoolean<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayBoolean::xor(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<T: CDatatype, A: NDArray<DType = T>, O: NDArray<DType = T>> NDArrayBoolean<O> for A {}

pub trait NDArrayAbs: NDArray + Sized {
    fn abs(self) -> Result<ArrayOp<ArrayUnary<Self::DType, Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::abs(self)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayAbs for A {}

pub trait NDArrayExp: NDArray + Sized {
    fn exp(self) -> Result<ArrayOp<ArrayUnary<Self::DType, Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::exp(self)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayExp for A {}

pub trait NDArrayMath: NDArray + Sized {
    fn add<O>(self, rhs: O) -> Result<ArrayOp<ArrayDual<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + Sized,
    {
        let shape = check_shape(self.shape(), rhs.shape())?;
        let op = ArrayDual::add(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn div<O>(self, rhs: O) -> Result<ArrayOp<ArrayDual<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + Sized,
    {
        let shape = check_shape(self.shape(), rhs.shape())?;
        let op = ArrayDual::div(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn mul<O>(self, rhs: O) -> Result<ArrayOp<ArrayDual<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + Sized,
    {
        let shape = check_shape(self.shape(), rhs.shape())?;
        let op = ArrayDual::mul(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn rem<O>(self, rhs: O) -> Result<ArrayOp<ArrayDual<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + Sized,
    {
        let shape = check_shape(self.shape(), rhs.shape())?;
        let op = ArrayDual::rem(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn sub<O>(self, rhs: O) -> Result<ArrayOp<ArrayDual<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + Sized,
    {
        let shape = check_shape(self.shape(), rhs.shape())?;
        let op = ArrayDual::sub(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn log<O>(self, base: O) -> Result<ArrayOp<ArrayDualFloat<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = f64> + Sized,
    {
        let shape = check_shape(self.shape(), base.shape())?;
        let op = ArrayDualFloat::log(self, base)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn pow<O>(self, exp: O) -> Result<ArrayOp<ArrayDualFloat<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = f64> + Sized,
    {
        let shape = check_shape(self.shape(), exp.shape())?;
        let op = ArrayDualFloat::pow(self, exp)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayMath for A {}

pub trait NDArrayMathScalar: NDArray + Sized {
    fn add_scalar(
        self,
        rhs: Self::DType,
    ) -> Result<ArrayOp<ArrayScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalar::add(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn div_scalar(
        self,
        rhs: Self::DType,
    ) -> Result<ArrayOp<ArrayScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalar::div(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn mul_scalar(
        self,
        rhs: Self::DType,
    ) -> Result<ArrayOp<ArrayScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalar::mul(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn rem_scalar(
        self,
        rhs: Self::DType,
    ) -> Result<ArrayOp<ArrayScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalar::rem(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn sub_scalar(
        self,
        rhs: Self::DType,
    ) -> Result<ArrayOp<ArrayScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalar::sub(self, rhs)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn log_scalar(self, base: f64) -> Result<ArrayOp<ArrayScalarFloat<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalarFloat::log(self, base)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn pow_scalar(self, exp: f64) -> Result<ArrayOp<ArrayScalarFloat<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayScalarFloat::pow(self, exp)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayMathScalar for A {}

pub trait NDArrayNumeric: NDArray + Sized
where
    Self::DType: Float,
{
    fn is_inf(self) -> Result<ArrayOp<ArrayUnary<Self::DType, u8, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn is_nan(self) -> Result<ArrayOp<ArrayUnary<Self::DType, u8, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self).expect("op");
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayNumeric for A where A::DType: Float {}

pub trait NDArrayTrig: NDArray + Sized {
    fn asin(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn sin(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn sinh(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn acos(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn cos(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn cosh(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn atan(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn tan(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;

    fn tanh(&self) -> ArrayOp<ArrayUnary<Self::DType, <Self::DType as CDatatype>::Float, Self>>;
}

// TODO: implement trigonometry methods

pub trait NDArrayCast: NDArray + Sized {
    fn cast<O: CDatatype>(self) -> Result<ArrayOp<ArrayCast<Self, O>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCast::new(self)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayCast for A {}

pub trait NDArrayCompare<O: NDArray<DType = Self::DType>>: NDArray + Sized {
    fn eq(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::eq(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn gt(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::gt(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn ge(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::ge(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn lt(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::lt(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn le(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::le(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn ne(self, other: O) -> Result<ArrayOp<ArrayCompare<Self::DType, Self, O>>, Error> {
        let shape = check_shape(self.shape(), other.shape())?;
        let op = ArrayCompare::ne(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray, O: NDArray> NDArrayCompare<O> for A where O: NDArray<DType = A::DType> {}

pub trait NDArrayCompareScalar: NDArray + Sized {
    fn eq_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::eq(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn gt_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::gt(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn ge_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::ge(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn lt_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::lt(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn le_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error> {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::le(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }

    fn ne_scalar(
        self,
        other: Self::DType,
    ) -> Result<ArrayOp<ArrayCompareScalar<Self::DType, Self>>, Error>
    where
        Self: Sized,
    {
        let shape = self.shape().to_vec();
        let op = ArrayCompareScalar::ne(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray> NDArrayCompareScalar for A {}

pub trait MatrixMath: NDArray + fmt::Debug {
    fn diagonal(self) -> Result<ArrayOp<MatDiag<Self>>, Error>
    where
        Self: Sized,
    {
        if self.ndim() >= 2 && self.shape()[self.ndim() - 1] == self.shape()[self.ndim() - 2] {
            let shape = self.shape().iter().take(self.ndim() - 1).copied().collect();
            let op = MatDiag::new(self)?;
            Ok(ArrayOp::new(shape, op))
        } else {
            Err(Error::Bounds(format!(
                "diagonal requires a square matrix, not {:?}",
                self
            )))
        }
    }

    fn matmul<O>(self, other: O) -> Result<ArrayOp<MatMul<Self::DType, Self, O>>, Error>
    where
        O: NDArray<DType = Self::DType> + fmt::Debug,
        Self: Sized,
    {
        if self.ndim() < 2 || other.ndim() < 2 {
            return Err(Error::Bounds(format!(
                "invalid matrices for matmul: {:?} and {:?}",
                self, other
            )));
        }

        let ndim = self.ndim();
        let prefix = &self.shape()[..ndim - 2];

        if other.ndim() != ndim {
            return Err(Error::Bounds(format!(
                "matrix multiply expects at least two dimensions but found shapes {:?} and {:?}",
                self.shape(),
                other.shape()
            )));
        } else if &other.shape()[..ndim - 2] != prefix {
            return Err(Error::Bounds(format!(
                "matrix multiply requires the same batch shape, not {:?} and {:?}",
                prefix,
                &other.shape()[..ndim - 2]
            )));
        }

        let a = self.shape()[ndim - 2];
        let b = self.shape()[ndim - 1];
        let c = other.shape()[ndim - 1];

        if other.shape()[ndim - 2] != b {
            return Err(Error::Bounds(format!(
                "invalid dimensions for matrix multiply: {:?} and {:?}",
                self.shape(),
                other.shape()
            )));
        }

        let mut shape = Vec::with_capacity(ndim);
        shape.extend_from_slice(prefix);
        shape.push(a);
        shape.push(c);

        let op = MatMul::new(self, other)?;
        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArray + fmt::Debug> MatrixMath for A {}

pub trait NDArrayReduce: NDArrayRead + fmt::Debug {
    fn all(self) -> Result<bool, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.all(&queue)
    }

    fn any(self) -> Result<bool, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.any(&queue)
    }

    fn max(self) -> Result<Self::DType, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.max(&queue)
    }

    fn max_axis(self, axis: usize) -> Result<ArrayOp<ArrayReduceAxis<Self::DType, Self>>, Error> {
        let shape = reduce_axis(&self, axis)?;
        let op = ArrayReduceAxis::max(self, axis);
        Ok(ArrayOp::new(shape, op))
    }

    fn min(self) -> Result<Self::DType, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.min(&queue)
    }

    fn min_axis(self, axis: usize) -> Result<ArrayOp<ArrayReduceAxis<Self::DType, Self>>, Error> {
        let shape = reduce_axis(&self, axis)?;
        let op = ArrayReduceAxis::min(self, axis);
        Ok(ArrayOp::new(shape, op))
    }

    fn product(self) -> Result<Self::DType, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.product(&queue)
    }

    fn product_axis(
        self,
        axis: usize,
    ) -> Result<ArrayOp<ArrayReduceAxis<Self::DType, Self>>, Error> {
        let shape = reduce_axis(&self, axis)?;
        let op = ArrayReduceAxis::product(self, axis);
        Ok(ArrayOp::new(shape, op))
    }

    fn sum(self) -> Result<Self::DType, Error> {
        let queue = Queue::new(self.context().clone(), self.size())?;
        let buffer = self.read(&queue)?;
        buffer.sum(&queue)
    }

    fn sum_axis(self, axis: usize) -> Result<ArrayOp<ArrayReduceAxis<Self::DType, Self>>, Error> {
        if axis >= self.ndim() {
            return Err(Error::Bounds(format!(
                "axis {} is out of bounds for {:?}",
                axis, self
            )));
        }

        let shape = if self.ndim() == 1 {
            vec![1]
        } else {
            let mut shape = vec![0; self.ndim() - 1];
            shape[..axis].copy_from_slice(&self.shape()[..axis]);
            shape[axis..].copy_from_slice(&self.shape()[(axis + 1)..]);
            shape
        };

        let op = ArrayReduceAxis::sum(self, axis);

        Ok(ArrayOp::new(shape, op))
    }
}

impl<A: NDArrayRead + fmt::Debug> NDArrayReduce for A {}

pub trait NDArrayWhere: NDArray<DType = u8> + fmt::Debug {
    fn gather_cond<T, L, R>(
        self,
        then: L,
        or_else: R,
    ) -> Result<ArrayOp<GatherCond<Self, T, L, R>>, Error>
    where
        T: CDatatype,
        L: NDArray<DType = T> + fmt::Debug,
        R: NDArray<DType = T> + fmt::Debug,
        Self: Sized,
    {
        if self.shape() == then.shape() && self.shape() == or_else.shape() {
            let shape = self.shape().to_vec();
            let op = GatherCond::new(self, then, or_else)?;
            Ok(ArrayOp::new(shape, op))
        } else {
            Err(Error::Bounds(format!(
                "cannot gather from {:?} and {:?} conditionally based on {:?} (wrong shape)",
                then, or_else, self
            )))
        }
    }
}

impl<A: NDArray<DType = u8> + fmt::Debug> NDArrayWhere for A {}

pub trait NDArrayTransform: NDArray + fmt::Debug {
    type Broadcast: NDArray<DType = Self::DType> + NDArrayRead + NDArrayTransform;
    type Expand: NDArray<DType = Self::DType> + NDArrayRead + NDArrayTransform;
    type Reshape: NDArray<DType = Self::DType> + NDArrayRead + NDArrayTransform;
    type Slice: NDArray<DType = Self::DType> + NDArrayRead + NDArrayTransform;
    type Transpose: NDArray<DType = Self::DType> + NDArrayRead + NDArrayTransform;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error>;

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error>;

    fn reshape(&self, shape: Shape) -> Result<Self::Reshape, Error>;

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error>;

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error>;
}

#[derive(Clone)]
pub enum AxisBound {
    At(usize),
    In(usize, usize, usize),
    Of(Vec<usize>),
}

impl AxisBound {
    pub fn size(&self) -> usize {
        match self {
            Self::At(_) => 0,
            Self::In(start, stop, step) => (stop - start) / step,
            Self::Of(indices) => indices.len(),
        }
    }
}

impl From<usize> for AxisBound {
    fn from(i: usize) -> Self {
        Self::At(i)
    }
}

impl From<Range<usize>> for AxisBound {
    fn from(range: Range<usize>) -> Self {
        Self::In(range.start, range.end, 1)
    }
}

impl From<Vec<usize>> for AxisBound {
    fn from(indices: Vec<usize>) -> Self {
        Self::Of(indices)
    }
}

impl fmt::Debug for AxisBound {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::At(i) => write!(f, "{}", i),
            Self::In(start, stop, 1) => write!(f, "{}:{}", start, stop),
            Self::In(start, stop, step) => write!(f, "{}:{}:{}", start, stop, step),
            Self::Of(indices) => write!(f, "{:?}", indices),
        }
    }
}

#[inline]
pub fn broadcast_shape(left: &[usize], right: &[usize]) -> Result<Shape, Error> {
    if left.is_empty() || right.is_empty() {
        return Err(Error::Bounds("cannot broadcast empty shape".to_string()));
    } else if left.len() < right.len() {
        return broadcast_shape(right, left);
    }

    let mut shape = vec![0; left.len()];
    let offset = left.len() - right.len();

    for x in 0..offset {
        shape[x] = left[x];
    }

    for x in 0..right.len() {
        if right[x] == 1 || right[x] == left[x + offset] {
            shape[x + offset] = left[x + offset];
        } else if left[x + offset] == 1 {
            shape[x + offset] = right[x];
        } else {
            return Err(Error::Bounds(format!(
                "cannot broadcast dimensions {} and {}",
                left[x + offset],
                right[x]
            )));
        }
    }

    debug_assert!(!shape.iter().any(|dim| *dim == 0));

    Ok(shape)
}

#[inline]
fn check_shape(left: &[usize], right: &[usize]) -> Result<Shape, Error> {
    if left == right {
        Ok(left.to_vec())
    } else {
        const MSG: &str = "this operation expects arrays of the same shape";
        const HINT: &str = "consider calling broadcast() explicitly";
        Err(Error::Bounds(format!(
            "{} but found {:?} and {:?} ({})",
            MSG, left, right, HINT
        )))
    }
}

#[cfg(feature = "opencl")]
#[inline]
fn div_ceil(num: usize, denom: usize) -> usize {
    if num % denom == 0 {
        num / denom
    } else {
        (num / denom) + 1
    }
}

#[inline]
fn reduce_axis<A: NDArray + fmt::Debug>(source: &A, axis: usize) -> Result<Shape, Error> {
    if axis >= source.ndim() {
        return Err(Error::Bounds(format!(
            "axis {} is out of bounds for {:?}",
            axis, source
        )));
    }

    if source.ndim() == 1 {
        Ok(vec![1])
    } else {
        let mut shape = Vec::with_capacity(source.ndim() - 1);
        shape.extend(source.shape().iter().take(axis).copied());
        shape.extend(source.shape().iter().skip(axis + 1).copied());
        Ok(shape)
    }
}

#[inline]
fn strides_for(shape: &[usize], ndim: usize) -> Vec<usize> {
    debug_assert!(ndim >= shape.len());

    let zeros = std::iter::repeat(0).take(ndim - shape.len());

    let strides = shape.iter().enumerate().map(|(x, dim)| {
        if *dim == 1 {
            0
        } else {
            shape.iter().rev().take(shape.len() - 1 - x).product()
        }
    });

    zeros.chain(strides).collect()
}
