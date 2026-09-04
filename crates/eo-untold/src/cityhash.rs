//! CityHash64 compatibility used by the frozen Untold reference.
//!
//! This is the CityHash64 variant used by Azahar and mirrored by
//! `eouhd/cityhash64.py`. It hashes the exact encoded base-level texture bytes,
//! never decoded RGBA pixels or container padding.

const K0: u64 = 0xC3A5_C85C_97CB_3127;
const K1: u64 = 0xB492_B66F_BE98_F273;
const K2: u64 = 0x9AE1_6A3B_2F90_404F;
const KMUL: u64 = 0x9DDF_EA08_EB38_2D69;

pub fn cityhash64(data: &[u8]) -> u64 {
    let len = data.len();
    if len <= 32 {
        return if len <= 16 {
            hash_len_0_to_16(data)
        } else {
            hash_len_17_to_32(data)
        };
    }
    if len <= 64 {
        return hash_len_33_to_64(data);
    }

    let mut x = fetch64(data, len - 40);
    let mut y = fetch64(data, len - 16).wrapping_add(fetch64(data, len - 56));
    let mut z = hash_len_16(
        fetch64(data, len - 48).wrapping_add(len as u64),
        fetch64(data, len - 24),
    );
    let mut v = weak_hash_len_32_with_seeds(data, len - 64, len as u64, z);
    let mut w = weak_hash_len_32_with_seeds(data, len - 32, y.wrapping_add(K1), x);
    x = x.wrapping_mul(K1).wrapping_add(fetch64(data, 0));

    let mut remaining = (len - 1) & !63;
    let mut offset = 0usize;
    while remaining != 0 {
        x = rotate(
            x.wrapping_add(y)
                .wrapping_add(v.0)
                .wrapping_add(fetch64(data, offset + 8)),
            37,
        )
        .wrapping_mul(K1);
        y = rotate(
            y.wrapping_add(v.1)
                .wrapping_add(fetch64(data, offset + 48)),
            42,
        )
        .wrapping_mul(K1);
        x ^= w.1;
        y = y
            .wrapping_add(v.0)
            .wrapping_add(fetch64(data, offset + 40));
        z = rotate(z.wrapping_add(w.0), 33).wrapping_mul(K1);
        v = weak_hash_len_32_with_seeds(
            data,
            offset,
            v.1.wrapping_mul(K1),
            x.wrapping_add(w.0),
        );
        w = weak_hash_len_32_with_seeds(
            data,
            offset + 32,
            z.wrapping_add(w.1),
            y.wrapping_add(fetch64(data, offset + 16)),
        );
        std::mem::swap(&mut z, &mut x);
        offset += 64;
        remaining -= 64;
    }

    hash_len_16(
        hash_len_16(v.0, w.0)
            .wrapping_add(shift_mix(y).wrapping_mul(K1))
            .wrapping_add(z),
        hash_len_16(v.1, w.1).wrapping_add(x),
    )
}

pub fn cityhash64_hex(data: &[u8]) -> String {
    format!("{:016X}", cityhash64(data))
}

fn fetch64(data: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(data[offset..offset + 8].try_into().expect("bounded CityHash read"))
}

fn fetch32(data: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(data[offset..offset + 4].try_into().expect("bounded CityHash read"))
}

fn rotate(value: u64, shift: u32) -> u64 {
    if shift == 0 {
        value
    } else {
        value.rotate_right(shift)
    }
}

fn shift_mix(value: u64) -> u64 {
    value ^ (value >> 47)
}

fn hash_128_to_64(low: u64, high: u64) -> u64 {
    let mut a = (low ^ high).wrapping_mul(KMUL);
    a ^= a >> 47;
    let mut b = (high ^ a).wrapping_mul(KMUL);
    b ^= b >> 47;
    b.wrapping_mul(KMUL)
}

fn hash_len_16(u: u64, v: u64) -> u64 {
    hash_128_to_64(u, v)
}

fn hash_len_16_mul(u: u64, v: u64, mul: u64) -> u64 {
    let mut a = (u ^ v).wrapping_mul(mul);
    a ^= a >> 47;
    let mut b = (v ^ a).wrapping_mul(mul);
    b ^= b >> 47;
    b.wrapping_mul(mul)
}

fn hash_len_0_to_16(data: &[u8]) -> u64 {
    let len = data.len();
    if len >= 8 {
        let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
        let a = fetch64(data, 0).wrapping_add(K2);
        let b = fetch64(data, len - 8);
        let c = rotate(b, 37).wrapping_mul(mul).wrapping_add(a);
        let d = rotate(a, 25).wrapping_add(b).wrapping_mul(mul);
        return hash_len_16_mul(c, d, mul);
    }
    if len >= 4 {
        let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
        let a = u64::from(fetch32(data, 0));
        return hash_len_16_mul(
            (len as u64).wrapping_add(a << 3),
            u64::from(fetch32(data, len - 4)),
            mul,
        );
    }
    if len > 0 {
        let a = u64::from(data[0]);
        let b = u64::from(data[len >> 1]);
        let c = u64::from(data[len - 1]);
        let y = a.wrapping_add(b << 8);
        let z = (len as u64).wrapping_add(c << 2);
        return shift_mix(y.wrapping_mul(K2) ^ z.wrapping_mul(K0)).wrapping_mul(K2);
    }
    K2
}

fn hash_len_17_to_32(data: &[u8]) -> u64 {
    let len = data.len();
    let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
    let a = fetch64(data, 0).wrapping_mul(K1);
    let b = fetch64(data, 8);
    let c = fetch64(data, len - 8).wrapping_mul(mul);
    let d = fetch64(data, len - 16).wrapping_mul(K2);
    hash_len_16_mul(
        rotate(a.wrapping_add(b), 43)
            .wrapping_add(rotate(c, 30))
            .wrapping_add(d),
        a.wrapping_add(rotate(b.wrapping_add(K2), 18))
            .wrapping_add(c),
        mul,
    )
}

fn weak_hash_len_32_with_seeds(
    data: &[u8],
    offset: usize,
    a: u64,
    b: u64,
) -> (u64, u64) {
    weak_hash_len_32_with_seeds_values(
        fetch64(data, offset),
        fetch64(data, offset + 8),
        fetch64(data, offset + 16),
        fetch64(data, offset + 24),
        a,
        b,
    )
}

fn weak_hash_len_32_with_seeds_values(
    w: u64,
    x: u64,
    y: u64,
    z: u64,
    mut a: u64,
    mut b: u64,
) -> (u64, u64) {
    a = a.wrapping_add(w);
    b = rotate(b.wrapping_add(a).wrapping_add(z), 21);
    let c = a;
    a = a.wrapping_add(x).wrapping_add(y);
    b = b.wrapping_add(rotate(a, 44));
    (a.wrapping_add(z), b.wrapping_add(c))
}

fn hash_len_33_to_64(data: &[u8]) -> u64 {
    let len = data.len();
    let mul = K2.wrapping_add((len as u64).wrapping_mul(2));
    let mut a = fetch64(data, 0).wrapping_mul(K2);
    let mut b = fetch64(data, 8);
    let c = fetch64(data, len - 24);
    let d = fetch64(data, len - 32);
    let e = fetch64(data, 16).wrapping_mul(K2);
    let f = fetch64(data, 24).wrapping_mul(9);
    let g = fetch64(data, len - 8);
    let h = fetch64(data, len - 16).wrapping_mul(mul);
    let u = rotate(a.wrapping_add(g), 43)
        .wrapping_add(rotate(b, 30).wrapping_add(c).wrapping_mul(9));
    let v = (a.wrapping_add(g) ^ d).wrapping_add(f).wrapping_add(1);
    let w = u
        .wrapping_add(v)
        .wrapping_mul(mul)
        .swap_bytes()
        .wrapping_add(h);
    let x = rotate(e.wrapping_add(f), 42).wrapping_add(c);
    let y = v
        .wrapping_add(w)
        .wrapping_mul(mul)
        .swap_bytes()
        .wrapping_add(g)
        .wrapping_mul(mul);
    let z = e.wrapping_add(f).wrapping_add(c);
    a = x
        .wrapping_add(z)
        .wrapping_mul(mul)
        .wrapping_add(y)
        .swap_bytes()
        .wrapping_add(b);
    b = shift_mix(
        z.wrapping_add(a)
            .wrapping_mul(mul)
            .wrapping_add(d)
            .wrapping_add(h),
    )
    .wrapping_mul(mul);
    b.wrapping_add(x)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_frozen_python_reference_vectors() {
        let vectors: &[(&[u8], &str)] = &[
            (b"", "9AE16A3B2F90404F"),
            (b"a", "B3454265B6DF75E3"),
            (b"hello", "B48BE5A931380CE8"),
        ];
        for (data, expected) in vectors {
            assert_eq!(cityhash64_hex(data), *expected);
        }

        assert_eq!(
            cityhash64_hex(&(0u8..16).collect::<Vec<_>>()),
            "0EFD25A0A34156D4"
        );
        assert_eq!(
            cityhash64_hex(&(0u8..32).collect::<Vec<_>>()),
            "1A9D8199972CDF49"
        );
        assert_eq!(
            cityhash64_hex(&(0u8..64).collect::<Vec<_>>()),
            "E99AB80F5EC7DCA5"
        );
        assert_eq!(
            cityhash64_hex(&(0u8..128).collect::<Vec<_>>()),
            "10B153630AF1F395"
        );
    }
}
