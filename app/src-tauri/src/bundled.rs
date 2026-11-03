//! The course the app ships with: every language's syllabus, forms table and number words, and
//! the pack lock, embedded at compile time from `content/`.

use std::collections::BTreeMap;

use parley_core::app::CourseFiles;
use parley_core::content::Lang;

/// The text of `content/packs.lock.toml`.
pub const PACKS_LOCK: &str = include_str!("../../../content/packs.lock.toml");

/// The bundled content of every language.
pub fn courses() -> BTreeMap<Lang, CourseFiles> {
    let course = |syllabus: &str, forms: &str, numbers: &str| CourseFiles {
        syllabus_yaml: syllabus.to_string(),
        forms_tsv: forms.to_string(),
        numbers_yaml: Some(numbers.to_string()),
    };
    BTreeMap::from([
        (
            Lang::Es,
            course(
                include_str!("../../../content/es/syllabus.yaml"),
                include_str!("../../../content/es/forms.tsv"),
                include_str!("../../../content/es/numbers.yaml"),
            ),
        ),
        (
            Lang::Fr,
            course(
                include_str!("../../../content/fr/syllabus.yaml"),
                include_str!("../../../content/fr/forms.tsv"),
                include_str!("../../../content/fr/numbers.yaml"),
            ),
        ),
        (
            Lang::De,
            course(
                include_str!("../../../content/de/syllabus.yaml"),
                include_str!("../../../content/de/forms.tsv"),
                include_str!("../../../content/de/numbers.yaml"),
            ),
        ),
    ])
}
