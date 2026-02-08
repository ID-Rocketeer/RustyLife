use crate::space::SimulationSpace;

#[derive(Clone)]
pub enum PatternSource {
    Builtin(fn(&SimulationSpace, i128, i128)),
    Rle(String),
}

#[derive(Clone)]
pub struct Pattern {
    pub name: String,
    pub description: String,
    pub source: PatternSource,
}

pub fn get_builtin_patterns() -> Vec<Pattern> {
    // Patterns are now loaded dynamically from the `patterns/` directory.
    // We return an empty vector here to avoid duplication.
    vec![]
}
