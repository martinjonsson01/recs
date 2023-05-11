use crate::{Cloud, Mass, Surface};
use ecs::systems::command_buffers::IntoBoxedComponentIter;
use gfx_plugin::rendering::Position;
use rand::{thread_rng, Rng};

pub fn random_cloud_components(_index: usize) -> impl IntoBoxedComponentIter {
    (
        Cloud {
            vapor_accumulation_rate: 1.0,
            drop_emit_area: Surface {
                width: 10.0,
                depth: 10.0,
            },
        },
        Position {
            point: [
                thread_rng().gen_range(-100.0..100.0),
                thread_rng().gen_range(10.0..100.0),
                thread_rng().gen_range(-100.0..100.0),
            ]
            .into(),
        },
        Mass(0.0),
    )
}

pub fn create_evenly_interspersed_clouds(max_clouds: u32) -> Vec<(Cloud, Position, Mass)> {
    let mut entity_components = vec![];

    let square_width = (max_clouds as f32).sqrt() as u32;
    for x in 0..square_width {
        let x = x as f32;
        for z in 0..square_width {
            let z = z as f32;
            entity_components.push((
                Cloud {
                    vapor_accumulation_rate: 1.0,
                    drop_emit_area: Surface {
                        width: 10.0,
                        depth: 10.0,
                    },
                },
                Position {
                    // It's important to place the clouds at a low height so the simulation
                    // gets to the point of removing entities after only a few ticks.
                    point: [x * 10.0, 10.0, z * 10.0].into(),
                },
                Mass(0.0),
            ));
        }
    }

    entity_components
}
