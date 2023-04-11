use std::fmt;
use std::ops::Add;

use ocl::{Buffer, Context, Device, OclPrm, Platform, ProQue, Queue};

use super::ops::*;
use super::{
    broadcast_shape, AxisBound, CDatatype, Error, NDArray, NDArrayCompare, NDArrayRead,
    NDArrayReduce, Shape,
};

pub struct ArrayBase<T> {
    data: Vec<T>,
    shape: Shape,
}

impl<T: OclPrm + CDatatype> ArrayBase<T> {
    pub fn concatenate(arrays: Vec<Array<T>>, axis: usize) -> Result<Self, Error> {
        todo!()
    }

    pub fn constant(value: T, shape: Shape) -> Result<Self, Error> {
        let size = shape.iter().product();
        Ok(Self {
            data: vec![value; size],
            shape,
        })
    }

    pub fn eye(shape: Shape) -> Result<Self, Error> {
        let ndim = shape.len();
        if shape.len() < 2 || shape[ndim - 1] != shape[ndim - 2] {
            Err(Error::Bounds(format!(
                "invalid shape for identity matrix: {:?}",
                shape
            )))
        } else {
            todo!()
        }
    }

    pub fn random(shape: Shape) -> Result<Self, Error> {
        todo!()
    }
}

impl<T: OclPrm> NDArray for ArrayBase<T> {
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<'a, T: OclPrm> NDArray for &'a ArrayBase<T> {
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<'a, T: OclPrm> Add for &'a ArrayBase<T> {
    type Output = ArrayOp<ArrayAdd<Self, Self>>;

    fn add(self, rhs: Self) -> Self::Output {
        let shape = broadcast_shape(self.shape(), rhs.shape()).expect("add");
        let op = ArrayAdd::new(self, rhs);
        ArrayOp { op, shape }
    }
}

impl<T: CDatatype> NDArrayCompare<Self> for ArrayBase<T> {}

impl<'a, T: CDatatype> NDArrayCompare<Self> for &'a ArrayBase<T> {}

impl<T: CDatatype> NDArrayRead<T> for ArrayBase<T> {
    fn read(self, queue: Queue, output: Option<Buffer<T>>) -> Result<Buffer<T>, Error> {
        (&self).read(queue, output)
    }
}

impl<'a, T: CDatatype> NDArrayRead<T> for &'a ArrayBase<T> {
    fn read(self, queue: Queue, output: Option<Buffer<T>>) -> Result<Buffer<T>, Error> {
        let buffer = if let Some(output) = output {
            if output.len() == self.size() {
                Ok(output)
            } else {
                Err(Error::Bounds(format!(
                    "cannot buffer {:?} into {} elements",
                    self,
                    output.len()
                )))
            }
        } else {
            Buffer::<T>::builder()
                .queue(queue)
                .len(self.size())
                .build()
                .map_err(Error::from)
        }?;

        buffer.write(&self.data).enq()?;

        Ok(buffer)
    }
}

impl<T: CDatatype> NDArrayReduce<T> for ArrayBase<T> {
    fn all(&self) -> Result<bool, Error> {
        let platform = Platform::first()?;
        let device = Device::first(platform)?;
        let context = Context::builder()
            .platform(platform)
            .devices(device)
            .build()?;

        let queue = Queue::new(&context, device, None)?;
        let input = self.read(queue.clone(), None)?;

        kernels::reduce_all(queue, input).map_err(Error::from)
    }

    fn all_axis(&self, axis: usize) -> Result<ArrayOp<ArrayAll<Self>>, Error> {
        todo!()
    }

    fn any(&self) -> Result<bool, Error> {
        let platform = Platform::first()?;
        let device = Device::first(platform)?;
        let context = Context::builder()
            .platform(platform)
            .devices(device)
            .build()?;

        let queue = Queue::new(&context, device, None)?;
        let input = self.read(queue.clone(), None)?;

        kernels::reduce_any(queue, input).map_err(Error::from)
    }

    fn any_axis(&self, axis: usize) -> Result<ArrayOp<ArrayAny<Self>>, Error> {
        todo!()
    }

    fn max(&self) -> Result<T, Error> {
        todo!()
    }

    fn max_axis(&self, axis: usize) -> Result<ArrayOp<ArrayMax<Self>>, Error> {
        todo!()
    }

    fn min(&self) -> Result<T, Error> {
        todo!()
    }

    fn min_axis(&self, axis: usize) -> Result<ArrayOp<ArrayMin<Self>>, Error> {
        todo!()
    }

    fn product(&self) -> Result<T, Error> {
        todo!()
    }

    fn product_axis(&self, axis: usize) -> Result<ArrayOp<ArrayProduct<Self>>, Error> {
        todo!()
    }

    fn sum(&self) -> Result<T, Error> {
        todo!()
    }

    fn sum_axis(&self, axis: usize) -> Result<ArrayOp<ArraySum<Self>>, Error> {
        todo!()
    }
}

impl<T: CDatatype> fmt::Debug for ArrayBase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "array of {}s with shape {:?}", T::TYPE_STR, self.shape)
    }
}

pub struct ArrayOp<Op> {
    op: Op,
    shape: Shape,
}

impl<Op> ArrayOp<Op> {
    pub fn new(op: Op, shape: Shape) -> Self {
        Self { op, shape }
    }
}

impl<Op> NDArray for ArrayOp<Op> {
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

pub struct ArraySlice<A> {
    source: A,
    bounds: Vec<AxisBound>,
    shape: Shape,
}

impl<A> NDArray for ArraySlice<A> {
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

pub struct ArrayView<A> {
    source: A,
    strides: Vec<u64>,
    shape: Shape,
}

impl<A> NDArray for ArrayView<A> {
    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

pub enum Array<T: OclPrm> {
    Base(ArrayBase<T>),
    Slice(ArraySlice<Box<Self>>),
    View(ArrayView<Box<Self>>),
    Op(ArrayOp<Box<dyn super::ops::Op<T>>>),
}

impl<T: OclPrm> NDArray for Array<T> {
    fn shape(&self) -> &[usize] {
        match self {
            Self::Base(base) => base.shape(),
            Self::Slice(slice) => slice.shape(),
            Self::View(view) => view.shape(),
            Self::Op(op) => op.shape(),
        }
    }
}
