mod common;

use common::{errors, output_for};
use reze_compiler::{Code, Options};

#[test]
fn island_misuse_is_refused_with_its_code() {
    for (source, code) in [
        (
            "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"media\" />;",
            Code::IslandMediaMissing,
        ),
        (
            "import { Counter } from \"./Counter\";\nexport const view = <Counter island=\"sometimes\" />;",
            Code::IslandTrigger,
        ),
        ("export const view = <section island=\"visible\" />;", Code::IslandOnElement),
        (
            "import { Counter } from \"./Counter\";\nexport const view = <Counter islandMedia=\"(max-width: 40rem)\" />;",
            Code::IslandOrphan,
        ),
    ] {
        let diagnostics = errors(source, &Options::default());
        assert!(diagnostics.iter().any(|d| d.code == code), "{source}: {diagnostics:?}");
    }
}

#[test]
fn shared_island_stays_in_the_main_chunk_with_a_warning() {
    let found = output_for(
        "import { Counter } from \"./Counter\";\nexport const first = <Counter island=\"visible\" />;\nexport const second = <Counter step={1} />;",
        &Options::default(),
    )
    .diagnostics;
    assert!(found.iter().any(|d| d.code == Code::IslandNotSplit), "{found:?}");
}
