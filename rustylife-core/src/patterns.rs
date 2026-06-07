// Copyright (C) 2026 Steven P. Collins. All rights reserved.
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program.  If not, see <https://www.gnu.org/licenses/>.

use crate::space::Space;

#[derive(Clone)]
pub enum PatternSource<const N: usize = 4> {
    Builtin(fn(&Space<N>, i128, i128)),
    Rle(String),
}

#[derive(Clone)]
pub struct Pattern<const N: usize = 4> {
    pub name: String,
    pub description: String,
    pub source: PatternSource<N>,
}

pub fn get_builtin_patterns<const N: usize>() -> Vec<Pattern<N>> {
    // Patterns are now loaded dynamically from the `patterns/` directory.
    // We return an empty vector here to avoid duplication.
    vec![]
}
