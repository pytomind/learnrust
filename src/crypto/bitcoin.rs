// =============================================================================
// Bitcoin Cryptography & Address Encodings (Legacy, SegWit, Taproot)
// =============================================================================
use anyhow::{anyhow, Result};
use bech32::Hrp;
use ripemd::Ripemd160;
use secp256k1::PublicKey;
use sha2::{Digest, Sha256};

/// Compute HASH160 (RIPEMD160(SHA256(data)))
pub fn hash160(data: &[u8]) -> [u8; 20] {
    let sha = Sha256::digest(data);
    let mut ripemd = Ripemd160::new();
    ripemd.update(&sha);
    let result = ripemd.finalize();
    let mut out = [0u8; 20];
    out.copy_from_slice(&result);
    out
}

/// Bitcoin Legacy P2PKH Address (Base58Check with prefix '1')
pub fn pubkey_to_legacy_p2pkh(public_key: &PublicKey) -> String {
    let compressed = public_key.serialize();
    let h160 = hash160(&compressed);

    let mut payload = Vec::with_capacity(25);
    payload.push(0x00); // Mainnet P2PKH version byte
    payload.extend_from_slice(&h160);

    // Double SHA-256 for Base58Check 4-byte checksum
    let checksum = Sha256::digest(Sha256::digest(&payload));
    payload.extend_from_slice(&checksum[0..4]);

    bs58::encode(payload).into_string()
}

/// Bitcoin Native SegWit P2WPKH Address (Bech32 with prefix 'bc1q')
pub fn pubkey_to_segwit_p2wpkh(public_key: &PublicKey) -> Result<String> {
    let compressed = public_key.serialize();
    let h160 = hash160(&compressed);

    let hrp = Hrp::parse("bc").map_err(|e| anyhow!("Invalid HRP: {}", e))?;
    let addr = bech32::segwit::encode_v0(hrp, &h160)
        .map_err(|e| anyhow!("Bech32 encoding error: {}", e))?;

    Ok(addr)
}

/// Bitcoin Taproot P2TR Address (BIP-341 Bech32m with prefix 'bc1p')
pub fn pubkey_to_taproot_p2tr(public_key: &PublicKey) -> Result<String> {
    let (x_only, _) = public_key.x_only_public_key();
    let x_bytes = x_only.serialize();

    let hrp = Hrp::parse("bc").map_err(|e| anyhow!("Invalid HRP: {}", e))?;
    let addr = bech32::segwit::encode(hrp, bech32::segwit::VERSION_1, &x_bytes)
        .map_err(|e| anyhow!("Bech32m encoding error: {}", e))?;

    Ok(addr)
}

