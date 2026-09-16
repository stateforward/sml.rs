extern crate sml;

sml::sml_policies!(DuplicatePolicy {
    logger: (),
    logger: (),
});

fn main() {}
