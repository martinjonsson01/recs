use cgmath::*;
use rayon::prelude::*;

#[derive(Copy, Clone, Debug)]
struct Position(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Rotation(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Velocity(Vector3<f32>);

#[derive(Copy, Clone, Debug)]
struct Affine(Matrix4<f32>);

#[derive(Default)]
struct NaiveVecs {
    affines: Vec<Affine>,
    positions: Vec<Position>,
    rotations: Vec<Rotation>,
    velocities: Vec<Velocity>,
}

pub struct Benchmark(NaiveVecs);

impl Benchmark {
    pub fn new() -> Self {
        let mut naive_vecs = NaiveVecs::default();

        for _ in 0..1000 {
            naive_vecs
                .affines
                .push(Affine(Matrix4::<f32>::from_angle_x(Rad(1.2))));
            naive_vecs.positions.push(Position(Vector3::unit_x()));
            naive_vecs.rotations.push(Rotation(Vector3::unit_x()));
            naive_vecs.velocities.push(Velocity(Vector3::unit_x()));
        }

        Self(naive_vecs)
    }

    pub fn run(&mut self) {
        let Benchmark(naive_vecs) = self;

        let mut components: Vec<_> = naive_vecs
            .positions
            .iter_mut()
            .zip(&mut naive_vecs.affines)
            .collect();

        components.par_iter_mut().for_each(|(position, affine)| {
            for _ in 0..100 {
                affine.0 = affine.0.invert().unwrap();
            }

            position.0 = affine.0.transform_vector(position.0);
        });
    }
}
