#[derive(Debug)]
struct A(f32);

#[derive(Debug)]
struct B(f32);

pub struct Benchmark(Vec<Option<B>>);

impl Benchmark {
    pub fn new() -> Self {
        Self(vec![])
    }

    pub fn run(&mut self) {
        for i in &mut self.0 {
            _ = i.insert(B(0.0));
        }

        for i in &mut self.0 {
            _ = i.take();
        }
    }
}
