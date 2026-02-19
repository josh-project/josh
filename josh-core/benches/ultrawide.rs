use criterion::{Criterion, criterion_group, criterion_main};
use rand::Rng;
use rand::distr::{Alphabetic, Distribution};
use rand::rngs::ThreadRng;
use std::path::PathBuf;

use josh_core::filter::Filter;

const N_PATHS: usize = 3000;

fn generate_filters() -> Vec<Filter> {
    const PATH_COMPONENTS_MAX: usize = 10;
    const PATH_COMPONENT_LEN: usize = 2;

    // Create a single path component -- random lowercase characters,
    // length of PATH_COMPONENT_LEN
    fn make_path_component(rng: &mut ThreadRng) -> String {
        (0..PATH_COMPONENT_LEN)
            .map(|_| {
                let ch = Alphabetic.sample(rng) as char;
                ch.to_ascii_lowercase()
            })
            .collect()
    }

    // Create a single path -- anywhere from 1 to PATH_COMPONENTS_MAX components
    fn make_subdir_filter(rng: &mut ThreadRng) -> Filter {
        let num_components = rng.random_range(1..=PATH_COMPONENTS_MAX);
        let mut path = PathBuf::new();

        for _ in 0..num_components {
            path.push(make_path_component(rng))
        }

        Filter::new().subdir(&path).prefix(&path)
    }

    let mut rng = rand::rng();

    // Finally, create N_PATHS of random paths
    (0..N_PATHS).map(|_| make_subdir_filter(&mut rng)).collect()
}

fn ultrawide(c: &mut Criterion) {
    c.bench_function("ultrawide_filter_parse", |b| {
        b.iter_with_setup_wrapper(|runner| {
            let filter = generate_filters();

            runner.run(move || {
                let filter = josh_core::filter::compose(&filter);
                std::hint::black_box(filter);
            })
        });
    });
}

criterion_group!(benches, ultrawide);
criterion_main!(benches);
