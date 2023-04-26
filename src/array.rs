use std::cmp::Ordering;
use std::f32::consts::PI;
use std::ops::{Add, Div, Mul, Neg, Not, Rem, Sub};
use std::sync::{Arc, RwLock};
use std::{fmt, iter};

use rand::Rng;
use rayon::prelude::*;

use super::kernels;
use super::ops::*;
use super::{
    AxisBound, Buffer, CDatatype, Context, DeviceQueue, Error, Float, MatrixMath, NDArray,
    NDArrayAbs, NDArrayBoolean, NDArrayCast, NDArrayCompare, NDArrayCompareScalar, NDArrayExp,
    NDArrayMath, NDArrayMathScalar, NDArrayNumeric, NDArrayRead, NDArrayReduce, NDArrayTransform,
    NDArrayWrite, Queue, Shape,
};

#[derive(Clone)]
pub struct ArrayBase<T> {
    data: Arc<RwLock<Vec<T>>>,
    shape: Shape,
}

impl<T: CDatatype> ArrayBase<T> {
    pub fn copy<O: NDArrayRead<DType = T>>(other: &O) -> Result<Self, Error> {
        let shape = other.shape().to_vec();

        let context = Context::default()?;
        let queue = context.queue(other.size())?;
        let data = match other.read(queue)? {
            Buffer::CL(buffer) => {
                let mut data = vec![T::zero(); other.size()];
                buffer.read(&mut data).enq()?;
                data
            }
            Buffer::Host(data) => data,
        };

        Ok(Self {
            data: Arc::new(RwLock::new(data)),
            shape,
        })
    }

    fn new(shape: Shape, data: Vec<T>) -> Self {
        Self {
            data: Arc::new(RwLock::new(data)),
            shape,
        }
    }

    pub fn concatenate(arrays: Vec<Array<T>>, axis: usize) -> Result<Self, Error> {
        todo!()
    }

    pub fn constant(shape: Shape, value: T) -> Self {
        let size = shape.iter().product();
        Self::new(shape, vec![value; size])
    }

    pub fn from_vec(shape: Shape, data: Vec<T>) -> Result<Self, Error> {
        let size = shape.iter().product();
        if data.len() == size {
            Ok(Self::new(shape, data))
        } else {
            Err(Error::Bounds(format!(
                "{} data were provided for an array of size {}",
                data.len(),
                size
            )))
        }
    }

    pub fn to_vec(&self) -> Vec<T> {
        let data = self.data.read().expect("array data");
        data.to_vec()
    }
}

impl ArrayBase<f32> {
    pub fn random_normal(shape: Shape, seed: Option<usize>) -> Result<Self, Error> {
        let size = shape.iter().product();

        let context = Context::default()?;
        let queue = context.queue(size)?;

        let data = match queue.device_queue() {
            DeviceQueue::CL(cl_queue) => {
                let seed = seed.unwrap_or_else(|| {
                    let mut rng = rand::thread_rng();
                    rng.gen()
                });

                let buffer = kernels::random_normal(cl_queue.clone(), seed, size)?;

                let mut data = vec![0.; size];
                buffer.read(&mut data[..]).enq()?;
                data
            }
            DeviceQueue::CPU => {
                let mut u1 = vec![0.0f32; size];
                rand::thread_rng().fill(&mut u1[..]);

                let mut u2 = vec![0.0f32; size];
                rand::thread_rng().fill(&mut u2[..]);

                u1.into_par_iter()
                    .zip(u2.into_par_iter())
                    .map(|(u1, u2)| {
                        let r = (u1.ln() * -2.).sqrt();
                        let theta = 2. * PI * u2;
                        r * theta.cos()
                    })
                    .collect()
            }
        };

        Self::from_vec(shape, data)
    }

    // TODO: support mean and std_dev parameters
    pub fn random_uniform(shape: Shape, seed: Option<usize>) -> Result<Self, Error> {
        let size = shape.iter().product();
        let mut data = vec![0.; size];

        let context = Context::default()?;
        let queue = context.queue(size)?;

        match queue.device_queue() {
            DeviceQueue::CL(cl_queue) => {
                let seed = seed.unwrap_or_else(|| {
                    let mut rng = rand::thread_rng();
                    rng.gen()
                });

                let buffer = kernels::random_uniform(cl_queue.clone(), seed, size)?;

                buffer.read(&mut data[..]).enq()?;
            }
            DeviceQueue::CPU => rand::thread_rng().fill(&mut data[..]),
        }

        Self::from_vec(shape, data)
    }
}

impl<T: CDatatype> NDArray for ArrayBase<T> {
    type DType = T;

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<T: CDatatype> NDArrayAbs for ArrayBase<T> {}

impl<T: CDatatype> NDArrayExp for ArrayBase<T> {}

impl<T: CDatatype> NDArrayTransform for ArrayBase<T> {
    type Broadcast = ArrayView<Self>;
    type Expand = Self;
    type Reshape = Self;
    type Slice = ArraySlice<Self>;
    type Transpose = ArrayView<Self>;

    fn broadcast(&self, shape: Shape) -> Result<ArrayView<Self>, Error> {
        ArrayView::broadcast(self.clone(), shape)
    }

    fn expand_dim(&self, axis: usize) -> Result<Self::Expand, Error> {
        if axis > self.ndim() {
            return Err(Error::Bounds(format!(
                "cannot expand axis {} of {:?}",
                axis, self
            )));
        }

        let mut shape = Vec::with_capacity(self.ndim() + 1);
        shape.extend_from_slice(&self.shape);
        shape.insert(axis, 1);

        let data = self.data.clone();

        Ok(Self { data, shape })
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        todo!()
    }

    fn reshape(&self, shape: Shape) -> Result<Self, Error> {
        if shape.iter().product::<usize>() == self.size() {
            Ok(Self {
                shape,
                data: self.data.clone(),
            })
        } else {
            Err(Error::Bounds(format!(
                "cannot reshape from {:?} to {:?}",
                self.shape, shape
            )))
        }
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<ArraySlice<Self>, Error> {
        ArraySlice::new(self.clone(), bounds)
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<ArrayView<Self>, Error> {
        let axes = if let Some(axes) = axes {
            if axes.len() == self.ndim() && (0..self.ndim()).into_iter().all(|x| axes.contains(&x))
            {
                Ok(axes)
            } else {
                Err(Error::Bounds(format!(
                    "invalid permutation {:?} for shape {:?}",
                    axes, self.shape
                )))
            }
        } else {
            Ok((0..self.ndim()).into_iter().rev().collect())
        }?;

        let shape = axes.iter().copied().map(|x| self.shape[x]).collect();

        let source_strides = strides_for(&self.shape, self.ndim());
        let strides = axes.into_iter().map(|x| source_strides[x]).collect();

        Ok(ArrayView::new(self.clone(), shape, strides))
    }
}

impl<A: NDArrayRead> NDArrayBoolean<A> for ArrayBase<A::DType> {}

impl<I: CDatatype, O: CDatatype> NDArrayCast<O> for ArrayBase<I> {}

impl<T: CDatatype> NDArrayMath<ArrayBase<f64>> for ArrayBase<T> {}

impl<T: CDatatype, Op: super::ops::Op<Out = f64>> NDArrayMath<ArrayOp<Op>> for ArrayBase<T> {}

impl NDArrayNumeric for ArrayBase<f32> {
    fn is_inf(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self.clone());
        ArrayOp::new(op, shape)
    }

    fn is_nan(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self.clone());
        ArrayOp::new(op, shape)
    }
}

impl NDArrayNumeric for ArrayBase<f64> {
    fn is_inf(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self.clone());
        ArrayOp::new(op, shape)
    }

    fn is_nan(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self.clone());
        ArrayOp::new(op, shape)
    }
}

impl<T: CDatatype, A: NDArray<DType = f64>> NDArrayMath<ArraySlice<A>> for ArrayBase<T> {}

impl<T: CDatatype, A: NDArray<DType = f64>> NDArrayMath<ArrayView<A>> for ArrayBase<T> {}

impl<T: CDatatype> NDArrayMathScalar for ArrayBase<T> {}

macro_rules! impl_op {
    ($op:ident, $name:ident, $t:ty, $o:ty) => {
        impl<T: CDatatype, O> $op<$o> for $t
        where
            Self: NDArray,
            $o: NDArray,
        {
            type Output = ArrayOp<ArrayDual<T, Self, $o>>;

            fn $name(self, rhs: $o) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs);
                ArrayOp { op, shape }
            }
        }
    };
}

impl_op!(Add, add, ArrayBase<T>, ArrayBase<O>);
impl_op!(Div, div, ArrayBase<T>, ArrayBase<O>);
impl_op!(Mul, mul, ArrayBase<T>, ArrayBase<O>);
impl_op!(Rem, rem, ArrayBase<T>, ArrayBase<O>);
impl_op!(Sub, sub, ArrayBase<T>, ArrayBase<O>);

impl_op!(Add, add, ArrayBase<T>, ArrayOp<O>);
impl_op!(Div, div, ArrayBase<T>, ArrayOp<O>);
impl_op!(Mul, mul, ArrayBase<T>, ArrayOp<O>);
impl_op!(Rem, rem, ArrayBase<T>, ArrayOp<O>);
impl_op!(Sub, sub, ArrayBase<T>, ArrayOp<O>);

impl_op!(Add, add, ArrayBase<T>, ArraySlice<O>);
impl_op!(Div, div, ArrayBase<T>, ArraySlice<O>);
impl_op!(Mul, mul, ArrayBase<T>, ArraySlice<O>);
impl_op!(Rem, rem, ArrayBase<T>, ArraySlice<O>);
impl_op!(Sub, sub, ArrayBase<T>, ArraySlice<O>);

impl_op!(Add, add, ArrayBase<T>, ArrayView<O>);
impl_op!(Div, div, ArrayBase<T>, ArrayView<O>);
impl_op!(Mul, mul, ArrayBase<T>, ArrayView<O>);
impl_op!(Rem, rem, ArrayBase<T>, ArrayView<O>);
impl_op!(Sub, sub, ArrayBase<T>, ArrayView<O>);

macro_rules! impl_base_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype> $op<T> for ArrayBase<T> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs);
                ArrayOp::new(op, shape)
            }
        }
    };
}

impl_base_scalar_op!(Add, add);
impl_base_scalar_op!(Div, div);
impl_base_scalar_op!(Mul, mul);
impl_base_scalar_op!(Rem, rem);
impl_base_scalar_op!(Sub, sub);

impl<T: CDatatype> Neg for ArrayBase<T> {
    type Output = ArrayOp<ArrayUnary<T, <T as CDatatype>::Neg, Self>>;

    fn neg(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::neg(self);
        ArrayOp::new(op, shape)
    }
}

impl<T: CDatatype> Not for ArrayBase<T> {
    type Output = ArrayOp<ArrayUnary<T, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self);
        ArrayOp::new(op, shape)
    }
}

impl<A: NDArrayRead> MatrixMath<A> for ArrayBase<A::DType> {}

impl<T: CDatatype, A: NDArray<DType = T>> NDArrayCompare<A> for ArrayBase<T> {}

impl<T: CDatatype> NDArrayCompareScalar for ArrayBase<T> {}

impl<T: CDatatype> NDArrayRead for ArrayBase<T> {
    fn read(&self, queue: Queue) -> Result<Buffer<T>, Error> {
        let data = self.data.read().expect("array data");

        let buffer = match queue.device_queue() {
            DeviceQueue::CPU => data.to_vec().into(),
            DeviceQueue::CL(cl_queue) => {
                let buffer = ocl::Buffer::builder()
                    .queue(cl_queue.clone())
                    .len(self.size())
                    .build()?;

                buffer.write(data.as_slice()).enq()?;

                buffer.into()
            }
        };

        Ok(buffer)
    }
}

impl<A: NDArrayRead + fmt::Debug> NDArrayWrite<A> for ArrayBase<A::DType> {
    fn write(&self, other: &A) -> Result<(), Error> {
        if self.shape == other.shape() {
            let context = Context::default()?;
            let queue = context.queue(self.size())?;

            match other.read(queue)? {
                Buffer::CL(buffer) => {
                    let mut data = self.data.write().expect("data");
                    buffer.read(&mut data[..]).enq()?;
                }
                Buffer::Host(buffer) => {
                    let mut data = self.data.write().expect("data");
                    data.copy_from_slice(&buffer[..]);
                }
            }

            Ok(())
        } else {
            Err(Error::Bounds(format!(
                "cannot write {:?} to {:?}",
                other, self
            )))
        }
    }
}

impl<T: CDatatype> NDArrayReduce for ArrayBase<T> {}

impl<T: CDatatype> fmt::Debug for ArrayBase<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{} array with shape {:?}", T::TYPE_STR, self.shape)
    }
}

#[derive(Clone)]
pub struct ArrayOp<Op> {
    op: Op,
    shape: Shape,
}

impl<Op> ArrayOp<Op> {
    pub fn new(op: Op, shape: Shape) -> Self {
        Self { op, shape }
    }
}

impl<Op: super::ops::Op> NDArray for ArrayOp<Op> {
    type DType = Op::Out;

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<Op: super::ops::Op> NDArrayRead for ArrayOp<Op> {
    fn read(&self, queue: Queue) -> Result<Buffer<Op::Out>, Error> {
        self.op.enqueue(queue)
    }
}

impl<Op: super::ops::Op> NDArrayTransform for ArrayOp<Op>
where
    Self: Clone,
    Op: Clone,
{
    type Broadcast = ArrayView<Self>;
    type Expand = Self;
    type Reshape = Self;
    type Slice = ArraySlice<Self>;
    type Transpose = ArrayView<Self>;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error> {
        ArrayView::broadcast(self.clone(), shape)
    }

    fn expand_dim(&self, axis: usize) -> Result<Self::Expand, Error> {
        if axis > self.ndim() {
            return Err(Error::Bounds(format!(
                "cannot expand axis {} of {:?}",
                axis, self
            )));
        }

        let mut shape = Vec::with_capacity(self.ndim() + 1);
        shape.extend_from_slice(&self.shape);
        shape.insert(axis, 1);

        Ok(Self {
            op: self.op.clone(),
            shape,
        })
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        todo!()
    }

    fn reshape(&self, shape: Shape) -> Result<Self::Reshape, Error> {
        todo!()
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        ArraySlice::new(self.clone(), bounds)
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        ArrayView::transpose(self.clone(), axes)
    }
}

impl<Op: super::ops::Op> NDArrayCompareScalar for ArrayOp<Op> {}

impl<Op: super::ops::Op> NDArrayAbs for ArrayOp<Op> where Self: Clone {}

impl<Op: super::ops::Op> NDArrayExp for ArrayOp<Op> where Self: Clone {}

impl<Op: super::ops::Op> NDArrayNumeric for ArrayOp<Op>
where
    Op::Out: Float,
    Self: Clone,
{
    fn is_inf(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self.clone());
        ArrayOp::new(op, shape)
    }

    fn is_nan(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self.clone());
        ArrayOp::new(op, shape)
    }
}

impl<Op: super::ops::Op> NDArrayReduce for ArrayOp<Op> where Self: Clone {}

impl<Op: super::ops::Op, O: NDArray<DType = Op::Out>> MatrixMath<O> for ArrayOp<Op> {}

impl<Op: super::ops::Op, O: CDatatype> NDArrayCast<O> for ArrayOp<Op> {}

impl<A, Op> NDArrayBoolean<A> for ArrayOp<Op>
where
    A: NDArray,
    Op: super::ops::Op<Out = A::DType>,
{
}

impl<Op: super::ops::Op> NDArrayMathScalar for ArrayOp<Op> where Self: Clone {}

impl_op!(Add, add, ArrayOp<T>, ArrayBase<O>);
impl_op!(Div, div, ArrayOp<T>, ArrayBase<O>);
impl_op!(Mul, mul, ArrayOp<T>, ArrayBase<O>);
impl_op!(Rem, rem, ArrayOp<T>, ArrayBase<O>);
impl_op!(Sub, sub, ArrayOp<T>, ArrayBase<O>);

impl_op!(Add, add, ArrayOp<T>, ArrayOp<O>);
impl_op!(Div, div, ArrayOp<T>, ArrayOp<O>);
impl_op!(Mul, mul, ArrayOp<T>, ArrayOp<O>);
impl_op!(Rem, rem, ArrayOp<T>, ArrayOp<O>);
impl_op!(Sub, sub, ArrayOp<T>, ArrayOp<O>);

impl_op!(Add, add, ArrayOp<T>, ArraySlice<O>);
impl_op!(Div, div, ArrayOp<T>, ArraySlice<O>);
impl_op!(Mul, mul, ArrayOp<T>, ArraySlice<O>);
impl_op!(Rem, rem, ArrayOp<T>, ArraySlice<O>);
impl_op!(Sub, sub, ArrayOp<T>, ArraySlice<O>);

impl_op!(Add, add, ArrayOp<T>, ArrayView<O>);
impl_op!(Div, div, ArrayOp<T>, ArrayView<O>);
impl_op!(Mul, mul, ArrayOp<T>, ArrayView<O>);
impl_op!(Rem, rem, ArrayOp<T>, ArrayView<O>);
impl_op!(Sub, sub, ArrayOp<T>, ArrayView<O>);

macro_rules! impl_op_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, Op: super::ops::Op<Out = T>> $op<T> for ArrayOp<Op> {
            type Output = ArrayOp<ArrayScalar<Op::Out, Self>>;

            fn $name(self, rhs: Op::Out) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs);
                ArrayOp::new(op, shape)
            }
        }
    };
}

impl_op_scalar_op!(Add, add);
impl_op_scalar_op!(Mul, mul);
impl_op_scalar_op!(Div, div);
impl_op_scalar_op!(Rem, rem);
impl_op_scalar_op!(Sub, sub);

impl<Op: super::ops::Op> Neg for ArrayOp<Op> {
    type Output = ArrayOp<ArrayUnary<Op::Out, <Op::Out as CDatatype>::Neg, Self>>;

    fn neg(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::neg(self);
        ArrayOp::new(op, shape)
    }
}

impl<Op: super::ops::Op> Not for ArrayOp<Op> {
    type Output = ArrayOp<ArrayUnary<Op::Out, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self);
        ArrayOp::new(op, shape)
    }
}

impl<Op: super::ops::Op> fmt::Debug for ArrayOp<Op> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(
            f,
            "{} array result with shape {:?}",
            Op::Out::TYPE_STR,
            self.shape
        )
    }
}

#[derive(Clone)]
pub struct ArraySlice<A> {
    source: A,
    bounds: Vec<AxisBound>,
    shape: Shape,
}

impl<A: NDArray> ArraySlice<A> {
    pub fn new(source: A, mut bounds: Vec<AxisBound>) -> Result<Self, Error> {
        if bounds.len() > source.ndim() {
            return Err(Error::Bounds(format!(
                "shape {:?} does not support slice bounds {:?}",
                source.shape(),
                bounds
            )));
        }

        for (bound, dim) in bounds.iter().zip(source.shape()) {
            match bound {
                AxisBound::At(i) => check_bound(i, dim, true)?,
                AxisBound::In(start, stop, _step) => {
                    check_bound(start, dim, false)?;
                    check_bound(stop, dim, false)?;
                }
                AxisBound::Of(indices) => {
                    for i in indices {
                        check_bound(i, dim, true)?;
                    }
                }
            }
        }

        let tail_bounds = source
            .shape()
            .iter()
            .rev()
            .take(source.ndim() - bounds.len())
            .copied()
            .map(|dim| AxisBound::In(0, dim, 1))
            .rev();

        bounds.extend(tail_bounds);

        debug_assert_eq!(source.ndim(), bounds.len());

        let shape = bounds
            .iter()
            .map(|bound| bound.size())
            .filter(|size| *size > 0)
            .collect();

        Ok(Self {
            source,
            bounds,
            shape,
        })
    }
}

impl<A: NDArray> NDArray for ArraySlice<A> {
    type DType = A::DType;

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<A: NDArrayRead> NDArrayRead for ArraySlice<A> {
    fn read(&self, queue: Queue) -> Result<Buffer<Self::DType>, Error> {
        let dims = self.shape();
        let strides = strides_for(self.shape(), self.ndim());
        let source_strides = strides_for(self.source.shape(), self.source.ndim());

        let buffer = match self.source.read(queue)? {
            Buffer::CL(source) => {
                let cl_queue = source.default_queue().expect("queue").clone();
                let buffer = kernels::slice(
                    cl_queue,
                    &source,
                    self.shape(),
                    &strides,
                    &self.bounds,
                    &source_strides,
                )?;

                Buffer::CL(buffer)
            }
            Buffer::Host(source) => {
                let output = (0..self.size())
                    .into_par_iter()
                    .map(|offset_out| {
                        let coord = strides
                            .iter()
                            .zip(dims)
                            .map(|(stride, dim)| (offset_out / stride) % dim)
                            .collect::<Vec<usize>>();

                        let mut offset_in = 0;
                        let mut x = 0;
                        for (stride, bound) in source_strides.iter().zip(self.bounds.iter()) {
                            let i = match bound {
                                AxisBound::At(i) => *i,
                                AxisBound::In(start, stop, step) => {
                                    let i = start + (coord[x] * step);
                                    debug_assert!(i < *stop);
                                    x += 1;
                                    i
                                }
                                AxisBound::Of(indices) => {
                                    let i = indices[coord[x]];
                                    x += 1;
                                    i
                                }
                            };

                            offset_in += i * stride;
                        }

                        source[offset_in]
                    })
                    .collect();

                Buffer::Host(output)
            }
        };

        Ok(buffer)
    }
}

impl<A: NDArray + fmt::Debug> NDArrayTransform for ArraySlice<A>
where
    Self: Clone,
{
    type Broadcast = ArrayView<Self>;
    type Expand = ArrayView<Self>;
    type Reshape = ArrayView<Self>;
    type Slice = Self;
    type Transpose = ArrayView<Self>;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error> {
        todo!()
    }

    fn expand_dim(&self, axis: usize) -> Result<Self::Expand, Error> {
        if axis > self.ndim() {
            return Err(Error::Bounds(format!(
                "cannot expand axis {} of {:?}",
                axis, self
            )));
        }

        let mut shape = Vec::with_capacity(self.ndim() + 1);
        shape.extend_from_slice(&self.shape);
        shape.insert(axis, 1);

        let strides = strides_for(&shape, shape.len());

        Ok(ArrayView::new(self.clone(), shape, strides))
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        todo!()
    }

    fn reshape(&self, shape: Shape) -> Result<ArrayView<Self>, Error> {
        todo!()
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        todo!()
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        todo!()
    }
}

impl<A: NDArray, O: CDatatype> NDArrayCast<O> for ArraySlice<A> {}

impl<A, O> NDArrayBoolean<O> for ArraySlice<A>
where
    A: NDArray,
    O: NDArray<DType = A::DType>,
{
}

impl<A: NDArray> NDArrayAbs for ArraySlice<A> where Self: Clone {}

impl<A: NDArray> NDArrayExp for ArraySlice<A> where Self: Clone {}

impl<T, A, O> MatrixMath<O> for ArraySlice<A>
where
    T: CDatatype,
    A: NDArray<DType = T>,
    O: NDArray<DType = T>,
{
}

impl<A: NDArrayRead> NDArrayMathScalar for ArraySlice<A> where Self: Clone {}

impl<A: NDArrayRead> NDArrayNumeric for ArraySlice<A>
where
    Self::DType: Float,
    Self: Clone,
{
    fn is_inf(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self.clone());
        ArrayOp::new(op, shape)
    }

    fn is_nan(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self.clone());
        ArrayOp::new(op, shape)
    }
}

impl_op!(Add, add, ArraySlice<T>, ArrayBase<O>);
impl_op!(Div, div, ArraySlice<T>, ArrayBase<O>);
impl_op!(Mul, mul, ArraySlice<T>, ArrayBase<O>);
impl_op!(Rem, rem, ArraySlice<T>, ArrayBase<O>);
impl_op!(Sub, sub, ArraySlice<T>, ArrayBase<O>);

impl_op!(Add, add, ArraySlice<T>, ArrayOp<O>);
impl_op!(Div, div, ArraySlice<T>, ArrayOp<O>);
impl_op!(Mul, mul, ArraySlice<T>, ArrayOp<O>);
impl_op!(Rem, rem, ArraySlice<T>, ArrayOp<O>);
impl_op!(Sub, sub, ArraySlice<T>, ArrayOp<O>);

impl_op!(Add, add, ArraySlice<T>, ArraySlice<O>);
impl_op!(Div, div, ArraySlice<T>, ArraySlice<O>);
impl_op!(Mul, mul, ArraySlice<T>, ArraySlice<O>);
impl_op!(Rem, rem, ArraySlice<T>, ArraySlice<O>);
impl_op!(Sub, sub, ArraySlice<T>, ArraySlice<O>);

impl_op!(Add, add, ArraySlice<T>, ArrayView<O>);
impl_op!(Div, div, ArraySlice<T>, ArrayView<O>);
impl_op!(Mul, mul, ArraySlice<T>, ArrayView<O>);
impl_op!(Rem, rem, ArraySlice<T>, ArrayView<O>);
impl_op!(Sub, sub, ArraySlice<T>, ArrayView<O>);

macro_rules! impl_slice_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, A: NDArray<DType = T>> $op<T> for ArraySlice<A> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs);
                ArrayOp::new(op, shape)
            }
        }
    };
}

impl_slice_scalar_op!(Add, add);
impl_slice_scalar_op!(Div, div);
impl_slice_scalar_op!(Mul, mul);
impl_slice_scalar_op!(Rem, rem);
impl_slice_scalar_op!(Sub, sub);

impl<T: CDatatype, A: NDArrayRead<DType = T>> Neg for ArraySlice<A> {
    type Output = ArrayOp<ArrayUnary<T, T::Neg, Self>>;

    fn neg(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::neg(self);
        ArrayOp::new(op, shape)
    }
}

impl<A: NDArrayRead> Not for ArraySlice<A>
where
    Self: NDArray,
{
    type Output = ArrayOp<ArrayUnary<<Self as NDArray>::DType, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self);
        ArrayOp::new(op, shape)
    }
}

impl<A: fmt::Debug> fmt::Debug for ArraySlice<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "slice of {:?} with shape {:?}", self.source, self.shape)
    }
}

#[derive(Clone)]
pub struct ArrayView<A> {
    source: A,
    shape: Shape,
    strides: Vec<usize>,
}

impl<A: NDArray> ArrayView<A> {
    fn new(source: A, shape: Shape, strides: Vec<usize>) -> Self {
        Self {
            source,
            shape,
            strides,
        }
    }

    fn broadcast(source: A, shape: Shape) -> Result<Self, Error> {
        if shape.len() < source.ndim() {
            return Err(Error::Bounds(format!(
                "cannot broadcast {:?} into {:?}",
                source.shape(),
                shape
            )));
        }

        for (dim, bdim) in source
            .shape()
            .iter()
            .zip(&shape[shape.len() - source.ndim()..])
        {
            if dim == bdim || *dim == 1 {
                // ok
            } else {
                return Err(Error::Bounds(format!(
                    "cannot broadcast dimension {} into {}",
                    dim, bdim
                )));
            }
        }

        let strides = strides_for(source.shape(), shape.len());

        Ok(Self::new(source, shape, strides))
    }

    fn transpose(source: A, axes: Option<Vec<usize>>) -> Result<Self, Error>
    where
        A: fmt::Debug,
    {
        let axes = if let Some(axes) = axes {
            if axes.len() == source.ndim() && (0..source.ndim()).all(|x| axes.contains(&x)) {
                Ok(axes)
            } else {
                Err(Error::Bounds(format!(
                    "cannot transpose axes {:?} of {:?}",
                    axes, source
                )))
            }
        } else {
            Ok((0..source.ndim()).into_iter().rev().collect())
        }?;

        let source_strides = strides_for(source.shape(), source.ndim());

        let mut shape = Vec::with_capacity(source.ndim());
        let mut strides = Vec::with_capacity(source.ndim());
        for x in axes {
            shape.push(source.shape()[x]);
            strides.push(source_strides[x]);
        }

        debug_assert!(!shape.iter().any(|dim| *dim == 0));

        Ok(Self {
            source,
            shape,
            strides,
        })
    }
}

impl<A: NDArray> NDArray for ArrayView<A> {
    type DType = A::DType;

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<A: NDArrayRead> NDArrayRead for ArrayView<A> {
    fn read(&self, queue: Queue) -> Result<Buffer<Self::DType>, Error> {
        let source = self.source.read(queue)?;
        let buffer = match source {
            Buffer::CL(buffer) => {
                let cl_queue = buffer.default_queue().expect("queue").clone();
                let strides = strides_for(&self.shape, self.ndim());

                let buffer = if self.size() == self.source.size() {
                    kernels::reorder_inplace(cl_queue, buffer, &self.shape, &strides, &self.strides)
                } else {
                    kernels::reorder(cl_queue, buffer, &self.shape, &strides, &self.strides)
                }?;

                Buffer::CL(buffer)
            }
            Buffer::Host(buffer) => {
                let source_strides = &self.strides;
                let strides = strides_for(self.shape(), self.ndim());
                let dims = self.shape();
                debug_assert_eq!(strides.len(), dims.len());

                let buffer = (0..self.size())
                    .into_par_iter()
                    .map(|offset| {
                        strides
                            .iter()
                            .copied()
                            .zip(dims.iter().copied())
                            .map(|(stride, dim)| {
                                if stride == 0 {
                                    0
                                } else {
                                    (offset / stride) % dim
                                }
                            }) // coord
                            .zip(source_strides.iter().copied())
                            .map(|(i, source_stride)| i * source_stride) // source offset
                            .sum::<usize>()
                    })
                    .map(|source_offset| buffer[source_offset])
                    .collect();

                Buffer::Host(buffer)
            }
        };

        Ok(buffer)
    }
}

impl<A: NDArray, O: CDatatype> NDArrayCast<O> for ArrayView<A> {}

impl<A, O> NDArrayBoolean<O> for ArrayView<A>
where
    A: NDArray,
    O: NDArray<DType = A::DType>,
{
}

impl<A: NDArray> NDArrayAbs for ArrayView<A> where Self: Clone {}

impl<A: NDArray> NDArrayExp for ArrayView<A> where Self: Clone {}

impl<A: NDArrayRead> NDArrayMathScalar for ArrayView<A> where Self: Clone {}

impl<A: NDArrayRead> NDArrayNumeric for ArrayView<A>
where
    Self::DType: Float,
    Self: Clone,
{
    fn is_inf(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::inf(self.clone());
        ArrayOp::new(op, shape)
    }

    fn is_nan(&self) -> ArrayOp<ArrayUnary<Self::DType, u8, Self>> {
        let shape = self.shape().to_vec();
        let op = ArrayUnary::nan(self.clone());
        ArrayOp::new(op, shape)
    }
}

impl_op!(Add, add, ArrayView<T>, ArrayBase<O>);
impl_op!(Div, div, ArrayView<T>, ArrayBase<O>);
impl_op!(Mul, mul, ArrayView<T>, ArrayBase<O>);
impl_op!(Rem, rem, ArrayView<T>, ArrayBase<O>);
impl_op!(Sub, sub, ArrayView<T>, ArrayBase<O>);

impl_op!(Add, add, ArrayView<T>, ArrayOp<O>);
impl_op!(Div, div, ArrayView<T>, ArrayOp<O>);
impl_op!(Mul, mul, ArrayView<T>, ArrayOp<O>);
impl_op!(Rem, rem, ArrayView<T>, ArrayOp<O>);
impl_op!(Sub, sub, ArrayView<T>, ArrayOp<O>);

impl_op!(Add, add, ArrayView<T>, ArraySlice<O>);
impl_op!(Div, div, ArrayView<T>, ArraySlice<O>);
impl_op!(Mul, mul, ArrayView<T>, ArraySlice<O>);
impl_op!(Rem, rem, ArrayView<T>, ArraySlice<O>);
impl_op!(Sub, sub, ArrayView<T>, ArraySlice<O>);

impl_op!(Add, add, ArrayView<T>, ArrayView<O>);
impl_op!(Div, div, ArrayView<T>, ArrayView<O>);
impl_op!(Mul, mul, ArrayView<T>, ArrayView<O>);
impl_op!(Rem, rem, ArrayView<T>, ArrayView<O>);
impl_op!(Sub, sub, ArrayView<T>, ArrayView<O>);

macro_rules! impl_view_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, A: NDArray<DType = T>> $op<T> for ArrayView<A> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs);
                ArrayOp::new(op, shape)
            }
        }
    };
}

impl_view_scalar_op!(Add, add);
impl_view_scalar_op!(Div, div);
impl_view_scalar_op!(Mul, mul);
impl_view_scalar_op!(Rem, rem);
impl_view_scalar_op!(Sub, sub);

impl<A: NDArrayRead> Neg for ArrayView<A> {
    type Output = ArrayOp<ArrayUnary<A::DType, <A::DType as CDatatype>::Neg, Self>>;

    fn neg(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::neg(self);
        ArrayOp::new(op, shape)
    }
}

impl<A: NDArrayRead> Not for ArrayView<A> {
    type Output = ArrayOp<ArrayUnary<A::DType, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self);
        ArrayOp::new(op, shape)
    }
}

impl<A: NDArray + fmt::Debug> NDArrayTransform for ArrayView<A> {
    type Broadcast = Self;
    type Expand = Self;
    type Reshape = ArrayView<Self>;
    type Slice = ArraySlice<Self>;
    type Transpose = Self;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error> {
        todo!()
    }

    fn expand_dim(&self, axis: usize) -> Result<Self::Expand, Error> {
        todo!()
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        todo!()
    }

    fn reshape(&self, shape: Shape) -> Result<Self::Reshape, Error> {
        todo!()
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        todo!()
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        todo!()
    }
}

impl<T, A, O> MatrixMath<O> for ArrayView<A>
where
    T: CDatatype,
    A: NDArray<DType = T>,
    O: NDArray<DType = T>,
{
}

impl<A: fmt::Debug> fmt::Debug for ArrayView<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "view of {:?} with shape {:?}", self.source, self.shape)
    }
}

pub enum Array<T: CDatatype> {
    Base(ArrayBase<T>),
    Slice(ArraySlice<Box<Self>>),
    View(ArrayView<Box<Self>>),
    Op(ArrayOp<Box<dyn super::ops::Op<Out = T>>>),
}

impl<T: CDatatype> NDArray for Array<T> {
    type DType = T;

    fn shape(&self) -> &[usize] {
        match self {
            Self::Base(base) => &base.shape,
            Self::Slice(slice) => &slice.shape,
            Self::View(view) => &view.shape,
            Self::Op(op) => &op.shape,
        }
    }
}

#[inline]
fn check_bound(i: &usize, dim: &usize, is_index: bool) -> Result<(), Error> {
    match i.cmp(dim) {
        Ordering::Less => Ok(()),
        Ordering::Equal if !is_index => Ok(()),
        Ordering::Greater | Ordering::Equal => Err(Error::Bounds(format!(
            "index {i} is out of bounds for dimension {dim}"
        ))),
    }
}

#[inline]
fn strides_for(shape: &[usize], ndim: usize) -> Vec<usize> {
    debug_assert!(ndim >= shape.len());

    let zeros = iter::repeat(0).take(ndim - shape.len());

    let strides = shape.iter().enumerate().map(|(x, dim)| {
        if *dim == 1 {
            0
        } else {
            shape.iter().rev().take(shape.len() - 1 - x).product()
        }
    });

    zeros.chain(strides).collect()
}
