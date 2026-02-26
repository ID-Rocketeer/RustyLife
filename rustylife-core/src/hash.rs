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

/// Hashes a pair of coordinates into a bucket index based on the given bucket_count.
pub fn hash_coordinates(x: i128, y: i128, bucket_count: usize) -> usize {
    // A simple, fast hashing approach for coordinates.
    // We combine the bits of x and y and use a basic modulo.
    // Since i128 is 16 bytes, we can use some bitwise XORs to mix them.

    let h = x ^ (y.rotate_left(32));

    // Mix the resulting 128 bits down to 64 then 32 etc if needed,
    // but for a simple % bucket_count, even the lower bits or an XOR of all segments works.

    // Extract 64-bit chunks and XOR them
    let h_low = h as u64;
    let h_high = (h >> 64) as u64;

    let mix = h_low ^ h_high;

    // Mix further to ensure distribution
    let final_mix = mix ^ (mix >> 32) ^ (mix >> 16) ^ (mix >> 8);

    (final_mix % (bucket_count as u64)) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_distribution_basic() {
        // Ensure neighbor cells likely land in different buckets
        // (though collisions are allowed and handled by the BST)
        let h1 = hash_coordinates(0, 0, crate::BUCKET_COUNT);
        let h2 = hash_coordinates(1, 0, crate::BUCKET_COUNT);
        let h3 = hash_coordinates(0, 1, crate::BUCKET_COUNT);

        assert_ne!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_hash_stability() {
        assert_eq!(
            hash_coordinates(12345, -67890, crate::BUCKET_COUNT),
            hash_coordinates(12345, -67890, crate::BUCKET_COUNT)
        );
    }
}
