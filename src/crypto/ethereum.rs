// =============================================================================
// Ethereum Cryptography & EIP-55 Address Encoding
// =============================================================================
use secp256k1::PublicKey;
use tiny_keccak::{Hasher, Keccak};

/// Convert a secp256k1 public key to an EIP-55 checksummed Ethereum address
pub fn pubkey_to_eth_address(public_key: &PublicKey) -> String {
    let uncompressed = public_key.serialize_uncompressed(); // 65 bytes: [0x04, X (32 bytes), Y (32 bytes)]
    let pub_bytes = &uncompressed[1..65]; // Omit 0x04 prefix (64 bytes)

    // Ethereum uses Keccak-256 (not SHA3-256)
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(pub_bytes);
    hasher.finalize(&mut hash);

    // The Ethereum address is the last 20 bytes of the Keccak-256 hash
    let raw_addr = &hash[12..32];
    let hex_addr = hex::encode(raw_addr);

    to_eip55_checksum(&hex_addr)
}

/// Convert raw 20-byte private key output to EIP-55 checksummed address
pub fn to_eip55_checksum(hex_addr: &str) -> String {
    let lower = hex_addr.to_lowercase();
    let clean = lower.trim_start_matches("0x");

    // Keccak-256 of lowercase address hex string
    let mut hasher = Keccak::v256();
    let mut hash = [0u8; 32];
    hasher.update(clean.as_bytes());
    hasher.finalize(&mut hash);

    let hash_hex = hex::encode(hash);
    let mut result = String::with_capacity(42);
    result.push_str("0x");

    for (i, c) in clean.chars().enumerate() {
        let nibble = u8::from_str_radix(&hash_hex[i..i + 1], 16).unwrap_or(0);
        if nibble >= 8 {
            result.push(c.to_ascii_uppercase());
        } else {
            result.push(c);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_eip55_checksum() {
        let addr = "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed";
        assert_eq!(
            to_eip55_checksum(addr),
            "0x5aAeb6053F3E94C9b9A09f33669435E7Ef1BeAed"
        );
    }
}

