use std::cmp::Ordering;
use std::ops::{Add, Div, Mul, Neg, Not, Rem, Sub};
use std::{fmt, iter};

use rayon::prelude::*;

use super::ops::*;
use super::{
    strides_for, AxisBound, Buffer, CDatatype, Context, Error, NDArray, NDArrayRead,
    NDArrayTransform, NDArrayWrite, Queue, Shape,
};

#[derive(Clone)]
pub struct ArrayBase<T> {
    context: Context,
    shape: Shape,
    data: Vec<T>,
}

impl<T: CDatatype> ArrayBase<T> {
    pub fn copy<O: NDArrayRead<DType = T>>(other: &O) -> Result<Self, Error> {
        let context = other.context().clone();
        let queue = Queue::new(context.clone(), other.size())?;

        let shape = other.shape().to_vec();
        let data = other.to_vec(&queue)?;

        Ok(Self {
            context,
            data,
            shape,
        })
    }

    pub fn new(shape: Shape, data: Vec<T>) -> Result<Self, Error> {
        Context::default().and_then(|cxt| Self::with_context(cxt, shape, data))
    }

    pub fn with_context(context: Context, shape: Shape, data: Vec<T>) -> Result<Self, Error> {
        let size = shape.iter().product();
        if data.len() == size {
            Ok(Self {
                context,
                data,
                shape,
            })
        } else {
            Err(Error::Bounds(format!(
                "expected {} elements for shape {:?} but found {}",
                size,
                shape,
                data.len()
            )))
        }
    }

    pub fn into_data(self) -> Vec<T> {
        self.data
    }
}

impl<T: CDatatype> NDArray for ArrayBase<T> {
    type DType = T;

    fn context(&self) -> &Context {
        &self.context
    }

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<T: CDatatype> NDArrayTransform for ArrayBase<T> {
    type Broadcast = ArrayView<Self>;
    type Expand = Self;
    type Reshape = Self;
    type Slice = ArraySlice<Self>;
    type Transpose = ArrayView<Self>;

    fn broadcast(&self, shape: Shape) -> Result<ArrayView<Self>, Error> {
        ArrayView::broadcast(self.clone(), shape)
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        let shape = expand_dims(self, axes)?;

        Ok(Self {
            context: self.context.clone(),
            data: self.data.clone(),
            shape,
        })
    }

    fn reshape(&self, shape: Shape) -> Result<Self, Error> {
        if shape.iter().product::<usize>() == self.size() {
            Ok(Self {
                context: self.context.clone(),
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
        let axes = permutation(self, axes)?;

        let shape = axes.iter().copied().map(|x| self.shape[x]).collect();

        let source_strides = strides_for(&self.shape, self.ndim());
        let strides = axes.into_iter().map(|x| source_strides[x]).collect();

        ArrayView::new(self.clone(), shape, strides)
    }
}

macro_rules! impl_base_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype> $op<ArrayBase<T>> for ArrayBase<T> {
            type Output = ArrayOp<ArrayDual<T, Self, ArrayBase<T>>>;

            fn $name(self, rhs: ArrayBase<T>) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs).expect("op");
                ArrayOp { op, shape }
            }
        }
    };
}

impl_base_op!(Add, add);
impl_base_op!(Div, div);
impl_base_op!(Mul, mul);
impl_base_op!(Rem, rem);
impl_base_op!(Sub, sub);

macro_rules! impl_base_dual_op {
    ($op:ident, $name:ident, $o:ty) => {
        impl<T: CDatatype, O> $op<$o> for ArrayBase<T>
        where
            $o: NDArray<DType = T>,
        {
            type Output = ArrayOp<ArrayDual<T, Self, $o>>;

            fn $name(self, rhs: $o) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs).expect("op");
                ArrayOp { op, shape }
            }
        }
    };
}

impl_base_dual_op!(Add, add, ArrayOp<O>);
impl_base_dual_op!(Div, div, ArrayOp<O>);
impl_base_dual_op!(Mul, mul, ArrayOp<O>);
impl_base_dual_op!(Rem, rem, ArrayOp<O>);
impl_base_dual_op!(Sub, sub, ArrayOp<O>);

impl_base_dual_op!(Add, add, ArraySlice<O>);
impl_base_dual_op!(Div, div, ArraySlice<O>);
impl_base_dual_op!(Mul, mul, ArraySlice<O>);
impl_base_dual_op!(Rem, rem, ArraySlice<O>);
impl_base_dual_op!(Sub, sub, ArraySlice<O>);

impl_base_dual_op!(Add, add, ArrayView<O>);
impl_base_dual_op!(Div, div, ArrayView<O>);
impl_base_dual_op!(Mul, mul, ArrayView<O>);
impl_base_dual_op!(Rem, rem, ArrayView<O>);
impl_base_dual_op!(Sub, sub, ArrayView<O>);

macro_rules! impl_base_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype> $op<T> for ArrayBase<T> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs).expect("op");
                ArrayOp::new(shape, op)
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
        let op = ArrayUnary::neg(self).expect("op");
        ArrayOp::new(shape, op)
    }
}

impl<T: CDatatype> Not for ArrayBase<T> {
    type Output = ArrayOp<ArrayUnary<T, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self).expect("op");
        ArrayOp::new(shape, op)
    }
}

impl<T: CDatatype> NDArrayRead for ArrayBase<T> {
    #[allow(unused_variables)]
    fn read(&self, queue: &Queue) -> Result<Buffer<T>, Error> {
        // TODO: there must be a better way to do this
        #[cfg(feature = "opencl")]
        if let Some(cl_queue) = &queue.cl_queue {
            let buffer = ocl::Buffer::builder()
                .queue(cl_queue.clone())
                .len(self.data.len())
                .copy_host_slice(&self.data[..])
                .build()?;

            return Ok(Buffer::CL(buffer));
        }

        Ok(Buffer::Host(self.data.to_vec()))
    }
}

impl<A: NDArrayRead + fmt::Debug> NDArrayWrite<A> for ArrayBase<A::DType> {
    fn write(&mut self, other: &A) -> Result<(), Error> {
        if self.shape == other.shape() {
            let queue = Queue::new(self.context().clone(), self.size())?;

            match other.read(&queue)? {
                Buffer::Host(buffer) => {
                    self.data.copy_from_slice(&buffer[..]);
                }
                #[cfg(feature = "opencl")]
                Buffer::CL(buffer) => {
                    buffer.read(&mut self.data[..]).enq()?;
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
    pub fn new(shape: Shape, op: Op) -> Self {
        Self { op, shape }
    }
}

impl<Op: super::ops::Op> NDArray for ArrayOp<Op> {
    type DType = Op::Out;

    fn context(&self) -> &Context {
        self.op.context()
    }

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<Op: super::ops::Op> NDArrayRead for ArrayOp<Op> {
    fn read(&self, queue: &Queue) -> Result<Buffer<Op::Out>, Error> {
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

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        let shape = expand_dims(self, axes)?;
        self.reshape(shape)
    }

    fn reshape(&self, shape: Shape) -> Result<Self::Reshape, Error> {
        if shape.iter().product::<usize>() == self.size() {
            Ok(Self {
                shape,
                op: self.op.clone(),
            })
        } else {
            Err(Error::Bounds(format!(
                "cannot reshape {:?} into {:?} (wrong size)",
                self, shape
            )))
        }
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        ArraySlice::new(self.clone(), bounds)
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        let axes = permutation(self, axes)?;
        let shape = axes.iter().copied().map(|x| self.shape[x]).collect();
        let strides = strides_for(self.shape(), self.ndim());
        let strides = axes.into_iter().map(|x| strides[x]).collect();
        ArrayView::new(self.clone(), shape, strides)
    }
}

macro_rules! impl_op_dual_op {
    ($op:ident, $name:ident, $o:ty) => {
        impl<T: CDatatype, Op: super::ops::Op<Out = T>, O> $op<$o> for ArrayOp<Op>
        where
            $o: NDArray<DType = T>,
        {
            type Output = ArrayOp<ArrayDual<T, Self, $o>>;

            fn $name(self, rhs: $o) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs).expect("op");
                ArrayOp { op, shape }
            }
        }
    };
}

impl_op_dual_op!(Add, add, ArrayBase<O>);
impl_op_dual_op!(Div, div, ArrayBase<O>);
impl_op_dual_op!(Mul, mul, ArrayBase<O>);
impl_op_dual_op!(Rem, rem, ArrayBase<O>);
impl_op_dual_op!(Sub, sub, ArrayBase<O>);

impl_op_dual_op!(Add, add, ArrayOp<O>);
impl_op_dual_op!(Div, div, ArrayOp<O>);
impl_op_dual_op!(Mul, mul, ArrayOp<O>);
impl_op_dual_op!(Rem, rem, ArrayOp<O>);
impl_op_dual_op!(Sub, sub, ArrayOp<O>);

impl_op_dual_op!(Add, add, ArraySlice<O>);
impl_op_dual_op!(Div, div, ArraySlice<O>);
impl_op_dual_op!(Mul, mul, ArraySlice<O>);
impl_op_dual_op!(Rem, rem, ArraySlice<O>);
impl_op_dual_op!(Sub, sub, ArraySlice<O>);

impl_op_dual_op!(Add, add, ArrayView<O>);
impl_op_dual_op!(Div, div, ArrayView<O>);
impl_op_dual_op!(Mul, mul, ArrayView<O>);
impl_op_dual_op!(Rem, rem, ArrayView<O>);
impl_op_dual_op!(Sub, sub, ArrayView<O>);

macro_rules! impl_op_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, Op: super::ops::Op<Out = T>> $op<T> for ArrayOp<Op> {
            type Output = ArrayOp<ArrayScalar<Op::Out, Self>>;

            fn $name(self, rhs: Op::Out) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs).expect("op");
                ArrayOp::new(shape, op)
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
        let op = ArrayUnary::neg(self).expect("op");
        ArrayOp::new(shape, op)
    }
}

impl<Op: super::ops::Op> Not for ArrayOp<Op> {
    type Output = ArrayOp<ArrayUnary<Op::Out, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self).expect("op");
        ArrayOp::new(shape, op)
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
    strides: Vec<usize>,
    source_strides: Vec<usize>,
    #[cfg(feature = "opencl")]
    kernel_op: ocl::Program,
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
            .collect::<Vec<usize>>();

        let strides = strides_for(&shape, shape.len());
        let source_strides = strides_for(source.shape(), source.ndim());

        #[cfg(feature = "opencl")]
        let kernel_op = crate::cl_programs::slice::<A::DType>(
            source.context(),
            &shape,
            &strides,
            &bounds,
            &source_strides,
        )?;

        Ok(Self {
            source,
            bounds,
            shape,
            strides,
            source_strides,
            #[cfg(feature = "opencl")]
            kernel_op,
        })
    }

    fn read_vec(&self, source: Vec<A::DType>) -> Result<Vec<A::DType>, Error> {
        let output = (0..self.size())
            .into_par_iter()
            .map(|offset_out| {
                let coord = self
                    .strides
                    .iter()
                    .zip(&self.shape)
                    .map(|(stride, dim)| (offset_out / stride) % dim)
                    .collect::<Vec<usize>>();

                let mut offset_in = 0;
                let mut x = 0;
                for (stride, bound) in self.source_strides.iter().zip(self.bounds.iter()) {
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

        Ok(output)
    }

    #[cfg(feature = "opencl")]
    fn read_cl(&self, source: ocl::Buffer<A::DType>) -> Result<ocl::Buffer<A::DType>, Error> {
        let cl_queue = source.default_queue().expect("queue").clone();

        let output = ocl::Buffer::builder()
            .queue(cl_queue.clone())
            .len(self.size())
            .build()?;

        let kernel = ocl::Kernel::builder()
            .name("slice")
            .program(&self.kernel_op)
            .queue(cl_queue)
            .global_work_size(output.len())
            .arg(&source)
            .arg(&output)
            .build()?;

        unsafe { kernel.enq()? }

        Ok(output)
    }
}

impl<A: NDArray> NDArray for ArraySlice<A> {
    type DType = A::DType;

    fn context(&self) -> &Context {
        self.source.context()
    }

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<A: NDArrayRead> NDArrayRead for ArraySlice<A> {
    fn read(&self, queue: &Queue) -> Result<Buffer<Self::DType>, Error> {
        match self.source.read(queue)? {
            Buffer::Host(source) => self.read_vec(source).map(Buffer::Host),
            #[cfg(feature = "opencl")]
            Buffer::CL(source) => self.read_cl(source).map(Buffer::CL),
        }
    }
}

impl<A: NDArray + fmt::Debug> NDArrayTransform for ArraySlice<A>
where
    Self: Clone,
{
    type Broadcast = ArrayView<Self>;
    type Expand = ArrayView<Self>;
    type Reshape = ArrayView<Self>;
    type Slice = ArraySlice<Self>;
    type Transpose = ArrayView<Self>;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error> {
        ArrayView::broadcast(self.clone(), shape)
    }

    fn expand_dims(&self, axes: Vec<usize>) -> Result<Self::Expand, Error> {
        let shape = expand_dims(self, axes)?;
        let strides = strides_for(&shape, shape.len());
        ArrayView::new(self.clone(), shape, strides)
    }

    fn reshape(&self, shape: Shape) -> Result<ArrayView<Self>, Error> {
        if shape.iter().product::<usize>() == self.size() {
            let strides = strides_for(&shape, shape.len());
            ArrayView::new(self.clone(), shape, strides)
        } else {
            Err(Error::Bounds(format!(
                "cannot reshape {:?} into {:?}",
                self, shape
            )))
        }
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        ArraySlice::new(self.clone(), bounds)
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        let axes = permutation(self, axes)?;
        let shape = axes.iter().copied().map(|x| self.shape[x]).collect();
        let strides = axes.into_iter().map(|x| self.strides[x]).collect();
        ArrayView::new(self.clone(), shape, strides)
    }
}

macro_rules! impl_slice_dual_op {
    ($op:ident, $name:ident, $o:ty) => {
        impl<T: CDatatype, A: NDArray<DType = T>, O> $op<$o> for ArraySlice<A>
        where
            $o: NDArray<DType = T>,
        {
            type Output = ArrayOp<ArrayDual<T, Self, $o>>;

            fn $name(self, rhs: $o) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs).expect("op");
                ArrayOp { op, shape }
            }
        }
    };
}

impl_slice_dual_op!(Add, add, ArrayBase<O>);
impl_slice_dual_op!(Div, div, ArrayBase<O>);
impl_slice_dual_op!(Mul, mul, ArrayBase<O>);
impl_slice_dual_op!(Rem, rem, ArrayBase<O>);
impl_slice_dual_op!(Sub, sub, ArrayBase<O>);

impl_slice_dual_op!(Add, add, ArrayOp<O>);
impl_slice_dual_op!(Div, div, ArrayOp<O>);
impl_slice_dual_op!(Mul, mul, ArrayOp<O>);
impl_slice_dual_op!(Rem, rem, ArrayOp<O>);
impl_slice_dual_op!(Sub, sub, ArrayOp<O>);

impl_slice_dual_op!(Add, add, ArraySlice<O>);
impl_slice_dual_op!(Div, div, ArraySlice<O>);
impl_slice_dual_op!(Mul, mul, ArraySlice<O>);
impl_slice_dual_op!(Rem, rem, ArraySlice<O>);
impl_slice_dual_op!(Sub, sub, ArraySlice<O>);

impl_slice_dual_op!(Add, add, ArrayView<O>);
impl_slice_dual_op!(Div, div, ArrayView<O>);
impl_slice_dual_op!(Mul, mul, ArrayView<O>);
impl_slice_dual_op!(Rem, rem, ArrayView<O>);
impl_slice_dual_op!(Sub, sub, ArrayView<O>);

macro_rules! impl_slice_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, A: NDArray<DType = T>> $op<T> for ArraySlice<A> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs).expect("op");
                ArrayOp::new(shape, op)
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
        let op = ArrayUnary::neg(self).expect("op");
        ArrayOp::new(shape, op)
    }
}

impl<A: NDArrayRead> Not for ArraySlice<A>
where
    Self: NDArray,
{
    type Output = ArrayOp<ArrayUnary<<Self as NDArray>::DType, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self).expect("op");
        ArrayOp::new(shape, op)
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
    #[cfg(feature = "opencl")]
    kernel_op: ocl::Program,
}

impl<A: NDArray> ArrayView<A> {
    fn new(source: A, shape: Shape, strides: Vec<usize>) -> Result<Self, Error> {
        #[cfg(feature = "opencl")]
        let kernel_op = crate::cl_programs::reorder::<A::DType>(
            source.context(),
            &shape,
            &strides_for(&shape, shape.len()),
            &strides,
        )?;

        Ok(Self {
            source,
            shape,
            strides,
            #[cfg(feature = "opencl")]
            kernel_op,
        })
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

        Self::new(source, shape, strides)
    }

    fn read_vec(&self, source: Vec<A::DType>) -> Result<Vec<A::DType>, Error> {
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
            .map(|source_offset| source[source_offset])
            .collect();

        Ok(buffer)
    }

    #[cfg(feature = "opencl")]
    fn read_cl(&self, source: ocl::Buffer<A::DType>) -> Result<ocl::Buffer<A::DType>, Error> {
        let cl_queue = source.default_queue().expect("queue").clone();

        let output = ocl::Buffer::builder()
            .queue(cl_queue.clone())
            .len(self.size())
            .build()?;

        let kernel = ocl::Kernel::builder()
            .name("reorder")
            .program(&self.kernel_op)
            .queue(cl_queue)
            .global_work_size(output.len())
            .arg(&source)
            .arg(&output)
            .build()?;

        unsafe { kernel.enq()? }

        Ok(output)
    }
}

impl<A: NDArray> NDArray for ArrayView<A> {
    type DType = A::DType;

    fn context(&self) -> &Context {
        self.source.context()
    }

    fn shape(&self) -> &[usize] {
        &self.shape
    }
}

impl<A: NDArrayRead> NDArrayRead for ArrayView<A> {
    fn read(&self, queue: &Queue) -> Result<Buffer<Self::DType>, Error> {
        match self.source.read(queue)? {
            Buffer::Host(source) => self.read_vec(source).map(Buffer::Host),
            #[cfg(feature = "opencl")]
            Buffer::CL(source) => self.read_cl(source).map(Buffer::CL),
        }
    }
}

macro_rules! impl_view_dual_op {
    ($op:ident, $name:ident, $o:ty) => {
        impl<T: CDatatype, A: NDArray<DType = T>, O> $op<$o> for ArrayView<A>
        where
            $o: NDArray<DType = T>,
        {
            type Output = ArrayOp<ArrayDual<T, Self, $o>>;

            fn $name(self, rhs: $o) -> Self::Output {
                let shape = self.shape().to_vec();
                assert_eq!(shape, rhs.shape());

                let op = ArrayDual::$name(self, rhs).expect("op");
                ArrayOp { op, shape }
            }
        }
    };
}

impl_view_dual_op!(Add, add, ArrayBase<O>);
impl_view_dual_op!(Div, div, ArrayBase<O>);
impl_view_dual_op!(Mul, mul, ArrayBase<O>);
impl_view_dual_op!(Rem, rem, ArrayBase<O>);
impl_view_dual_op!(Sub, sub, ArrayBase<O>);

impl_view_dual_op!(Add, add, ArrayOp<O>);
impl_view_dual_op!(Div, div, ArrayOp<O>);
impl_view_dual_op!(Mul, mul, ArrayOp<O>);
impl_view_dual_op!(Rem, rem, ArrayOp<O>);
impl_view_dual_op!(Sub, sub, ArrayOp<O>);

impl_view_dual_op!(Add, add, ArraySlice<O>);
impl_view_dual_op!(Div, div, ArraySlice<O>);
impl_view_dual_op!(Mul, mul, ArraySlice<O>);
impl_view_dual_op!(Rem, rem, ArraySlice<O>);
impl_view_dual_op!(Sub, sub, ArraySlice<O>);

impl_view_dual_op!(Add, add, ArrayView<O>);
impl_view_dual_op!(Div, div, ArrayView<O>);
impl_view_dual_op!(Mul, mul, ArrayView<O>);
impl_view_dual_op!(Rem, rem, ArrayView<O>);
impl_view_dual_op!(Sub, sub, ArrayView<O>);

macro_rules! impl_view_scalar_op {
    ($op:ident, $name:ident) => {
        impl<T: CDatatype, A: NDArray<DType = T>> $op<T> for ArrayView<A> {
            type Output = ArrayOp<ArrayScalar<T, Self>>;

            fn $name(self, rhs: T) -> Self::Output {
                let shape = self.shape.to_vec();
                let op = ArrayScalar::$name(self, rhs).expect("op");
                ArrayOp::new(shape, op)
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
        let op = ArrayUnary::neg(self).expect("program");
        ArrayOp::new(shape, op)
    }
}

impl<A: NDArrayRead> Not for ArrayView<A> {
    type Output = ArrayOp<ArrayUnary<A::DType, u8, Self>>;

    fn not(self) -> Self::Output {
        let shape = self.shape.to_vec();
        let op = ArrayUnary::not(self).expect("program");
        ArrayOp::new(shape, op)
    }
}

impl<A: NDArray + Clone + fmt::Debug> NDArrayTransform for ArrayView<A>
where
    Self: Clone,
{
    type Broadcast = Self;
    type Expand = Self;
    type Reshape = ArrayView<Self>;
    type Slice = ArraySlice<Self>;
    type Transpose = Self;

    fn broadcast(&self, shape: Shape) -> Result<Self::Broadcast, Error> {
        if shape.len() < self.ndim() {
            return Err(Error::Bounds(format!(
                "cannot broadcast {:?} into {:?}",
                self, shape
            )));
        }

        let offset = shape.len() - self.ndim();
        let mut strides = Vec::with_capacity(shape.len());
        strides.extend(iter::repeat(0).take(offset));

        for (x, (dim, stride)) in self.shape().iter().copied().zip(&self.strides).enumerate() {
            if dim == 1 || dim == shape[offset + x] {
                strides.push(*stride);
            } else {
                return Err(Error::Bounds(format!(
                    "cannot broadcast {} into {}",
                    dim,
                    shape[offset + x]
                )));
            }
        }

        debug_assert_eq!(strides.len(), shape.len());

        ArrayView::new(self.source.clone(), shape, strides)
    }

    fn expand_dims(&self, mut axes: Vec<usize>) -> Result<Self::Expand, Error> {
        axes.sort();

        if axes.last().copied() > Some(self.ndim()) {
            return Err(Error::Bounds(format!(
                "cannot expand axes {:?} of {:?}",
                axes, self
            )));
        }

        let mut shape = Vec::with_capacity(self.ndim() + axes.len());
        shape.extend_from_slice(self.shape());

        let mut strides = Vec::with_capacity(self.ndim() + axes.len());
        strides.extend_from_slice(&self.strides);

        for x in axes.into_iter().rev() {
            shape.insert(x, 1);
            strides.insert(x, 0);
        }

        debug_assert_eq!(shape.len(), strides.len());

        ArrayView::new(self.source.clone(), shape, strides)
    }

    fn reshape(&self, shape: Shape) -> Result<Self::Reshape, Error> {
        let strides = strides_for(&shape, shape.len());
        ArrayView::new(self.clone(), shape, strides)
    }

    fn slice(&self, bounds: Vec<AxisBound>) -> Result<Self::Slice, Error> {
        ArraySlice::new(self.clone(), bounds)
    }

    fn transpose(&self, axes: Option<Vec<usize>>) -> Result<Self::Transpose, Error> {
        let axes = permutation(self, axes)?;
        let shape = axes.iter().copied().map(|x| self.shape[x]).collect();
        let strides = axes.into_iter().map(|x| self.strides[x]).collect();
        ArrayView::new(self.source.clone(), shape, strides)
    }
}

impl<A: fmt::Debug> fmt::Debug for ArrayView<A> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "view of {:?} with shape {:?}", self.source, self.shape)
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
fn expand_dims<A: NDArray + fmt::Debug>(source: &A, mut axes: Vec<usize>) -> Result<Shape, Error> {
    axes.sort();

    if axes.is_empty() {
        Ok(source.shape().to_vec())
    } else if *axes.last().expect("x") <= source.ndim() {
        let mut shape = Vec::with_capacity(source.ndim() + axes.len());
        shape.extend_from_slice(source.shape());

        for x in axes.into_iter().rev() {
            shape.insert(x, 1);
        }

        Ok(shape)
    } else {
        Err(Error::Bounds(format!(
            "cannot expand axes {:?} of {:?}",
            axes, source
        )))
    }
}

#[inline]
fn permutation<A: NDArray + fmt::Debug>(
    source: &A,
    axes: Option<Vec<usize>>,
) -> Result<Vec<usize>, Error> {
    let ndim = source.ndim();

    if let Some(axes) = axes {
        if axes.len() == ndim
            && axes.iter().copied().all(|x| x < ndim)
            && (0..ndim).into_iter().all(|x| axes.contains(&x))
        {
            Ok(axes)
        } else {
            Err(Error::Bounds(format!(
                "invalid permutation for {:?}: {:?}",
                source, axes
            )))
        }
    } else {
        Ok((0..ndim).into_iter().rev().collect())
    }
}
