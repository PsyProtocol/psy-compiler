use tiny_keccak::{Hasher as _, Keccak};

type Hash4 = [u64; 4];

fn keccak256_bytes_to_u32x8(bytes: &[u8]) -> [u32; 8] {
    let mut digest = [0u8; 32];
    let mut keccak = Keccak::v256();
    keccak.update(bytes);
    keccak.finalize(&mut digest);

    let mut limbs = [0u32; 8];
    for (i, chunk) in digest.chunks_exact(4).enumerate().take(8) {
        limbs[i] = u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]);
    }
    limbs
}

fn limbs8_to_hash(x: [u32; 8]) -> Hash4 {
    [
        (x[0] as u64) + ((x[1] as u64) << 32),
        (x[2] as u64) + ((x[3] as u64) << 32),
        (x[4] as u64) + ((x[5] as u64) << 32),
        (x[6] as u64) + ((x[7] as u64) << 32),
    ]
}

fn keccak_two_to_one(left: Hash4, right: Hash4) -> Hash4 {
    let mut bytes = Vec::with_capacity(64);
    for v in left.into_iter().chain(right) {
        bytes.extend_from_slice(&v.to_le_bytes());
    }
    limbs8_to_hash(keccak256_bytes_to_u32x8(&bytes))
}

fn main() {
    let mut zero_hashes_u64 = [[0u64; 4]; 32];
    let mut zero_hashes_u32 = [[0u32; 8]; 32];
    for i in 1..32 {
        zero_hashes_u64[i] = keccak_two_to_one(zero_hashes_u64[i - 1], zero_hashes_u64[i - 1]);
    }
    for (i, h) in zero_hashes_u64.iter().enumerate() {
        zero_hashes_u32[i] = [
            (h[0] & 0xffff_ffff) as u32,
            (h[0] >> 32) as u32,
            (h[1] & 0xffff_ffff) as u32,
            (h[1] >> 32) as u32,
            (h[2] & 0xffff_ffff) as u32,
            (h[2] >> 32) as u32,
            (h[3] & 0xffff_ffff) as u32,
            (h[3] >> 32) as u32,
        ];
    }

    println!("U32x8:");
    println!("[");
    for h in zero_hashes_u32 {
        println!(
            "    [{}, {}, {}, {}, {}, {}, {}, {}],",
            h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]
        );
    }
    println!("]");

    println!("\nDSL_assignments:");
    println!("let mut zs: [[u32; 8]; 32] = [[0, 0, 0, 0, 0, 0, 0, 0]; 32];");
    for (i, h) in zero_hashes_u32.iter().enumerate() {
        println!(
            "zs[{}] = [{}, {}, {}, {}, {}, {}, {}, {}];",
            i, h[0], h[1], h[2], h[3], h[4], h[5], h[6], h[7]
        );
    }
    println!("zs");

    println!("\nU64x4:");
    println!("[");
    for h in zero_hashes_u64 {
        println!("    [{}, {}, {}, {}],", h[0], h[1], h[2], h[3]);
    }
    println!("]");
}
