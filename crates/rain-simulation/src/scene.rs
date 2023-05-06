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
