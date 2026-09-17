extern crate sml;

use sml::{PolicyParts, Policies};

struct UserParts;
struct UserPolicies;

impl PolicyParts for UserParts {}

impl Policies for UserPolicies {}

fn main() {}
