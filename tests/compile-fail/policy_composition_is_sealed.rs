extern crate sml;

use sml::__private::PolicyParts;
use sml::Policy;

struct UserParts;
struct UserPolicies;

impl PolicyParts for UserParts {}

impl Policy for UserPolicies {}

fn main() {}
