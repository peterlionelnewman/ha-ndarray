pub fn add(left: usize, right: usize) -> usize {
    left + right
}

#[cfg(test)]
mod tests {
    use custos::CPU;
    use custos_math::Matrix;

    #[test]
    fn it_works() {
        let device = CPU::new();

        let a = Matrix::from((&device, (2, 3), [1., 2., 3., 4., 5., 6.]));
        let b = Matrix::from((&device, (3, 2), [6., 5., 4., 3., 2., 1.]));

        let c = a.gemm(&b);

        assert_eq!(c.read(), vec![20., 14., 56., 41.,]);
    }
}
