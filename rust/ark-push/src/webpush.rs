//! Web Push's message encryption (RFC 8291, `aes128gcm` of RFC 8188), both
//! ends: what a UnifiedPush message is sealed with, so the distributor that
//! carries it — FCM, ntfy, whoever the person chose — reads nothing of it.
//!
//! The device makes its [`Keys`] once (a P-256 key pair and a 16-byte auth
//! secret), keeps the private half, and hands the server the public key and
//! the secret beside its endpoint. The server [`seal`]s each message to
//! them with a key of its own made for that message; the device [`open`]s
//! it.

use base64::engine::general_purpose::URL_SAFE_NO_PAD as B64;
use base64::Engine;
use p256::elliptic_curve::sec1::ToEncodedPoint;
use p256::{PublicKey, SecretKey};
use ring::aead::{Aad, LessSafeKey, Nonce, UnboundKey, AES_128_GCM};
use ring::hkdf::{self, KeyType, Salt, HKDF_SHA256};

/// The record size every message is written in: one record, so a message
/// is at most this less the header's and the tag's bytes.
const RECORD: u32 = 4096;
/// The most a message may carry: what every push service takes (4 KiB of
/// body), less the header (86 bytes), the tag (16) and the delimiter.
pub const MAX_PLAINTEXT: usize = 3993;

/// Why a message could not be sealed or opened.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

fn err(s: impl Into<String>) -> Error {
    Error(s.into())
}

/// A device's Web Push keys: its P-256 key pair and its auth secret.
#[derive(Clone)]
pub struct Keys {
    secret: SecretKey,
    auth: [u8; 16],
}

impl std::fmt::Debug for Keys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Keys({})", self.p256dh())
    }
}

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    getrandom::getrandom(&mut b).expect("the system's random numbers");
    b
}

/// A P-256 secret key from the system's random numbers.
fn new_secret() -> SecretKey {
    loop {
        // A scalar of zero or past the order is all but impossible; drawn again.
        if let Ok(k) = SecretKey::from_slice(&random::<32>()) {
            return k;
        }
    }
}

fn uncompressed(p: &PublicKey) -> Vec<u8> {
    p.to_encoded_point(false).as_bytes().to_vec()
}

impl Keys {
    /// New keys, from the system's random numbers.
    pub fn generate() -> Keys {
        Keys {
            secret: new_secret(),
            auth: random(),
        }
    }

    /// The device's public key, as the server is handed it: the
    /// uncompressed point, base64url without padding.
    pub fn p256dh(&self) -> String {
        B64.encode(uncompressed(&self.secret.public_key()))
    }

    /// The auth secret, as the server is handed it.
    pub fn auth(&self) -> String {
        B64.encode(self.auth)
    }

    /// The keys as the device keeps them: the private scalar and the auth
    /// secret, base64url, `.`-separated. Kept where only the app reads it.
    pub fn to_private(&self) -> String {
        format!("{}.{}", B64.encode(self.secret.to_bytes()), B64.encode(self.auth))
    }

    /// The keys from what [`Keys::to_private`] wrote.
    pub fn from_private(text: &str) -> Result<Keys, Error> {
        let (k, a) = text.split_once('.').ok_or_else(|| err("not a key and a secret"))?;
        let k = B64.decode(k).map_err(|e| err(format!("the key: {e}")))?;
        let a = B64.decode(a).map_err(|e| err(format!("the secret: {e}")))?;
        Ok(Keys {
            secret: SecretKey::from_slice(&k).map_err(|_| err("not a P-256 key"))?,
            auth: a.try_into().map_err(|_| err("an auth secret is 16 bytes"))?,
        })
    }
}

/// An HKDF output's length, as ring asks it.
struct Len(usize);

impl KeyType for Len {
    fn len(&self) -> usize {
        self.0
    }
}

fn expand(prk: &hkdf::Prk, info: &[u8], len: usize) -> Vec<u8> {
    let mut out = vec![0u8; len];
    prk.expand(&[info], Len(len))
        .and_then(|okm| okm.fill(&mut out))
        .expect("an HKDF length within bounds");
    out
}

/// The content key and nonce of a message: RFC 8291 §3.4, from the shared
/// secret, the auth secret, both public keys and the message's salt.
fn content_keys(shared: &[u8], auth: &[u8], ua_public: &[u8], as_public: &[u8], salt: &[u8]) -> (Vec<u8>, Vec<u8>) {
    let prk_key = Salt::new(HKDF_SHA256, auth).extract(shared);
    let mut info = b"WebPush: info\0".to_vec();
    info.extend_from_slice(ua_public);
    info.extend_from_slice(as_public);
    let ikm = expand(&prk_key, &info, 32);
    let prk = Salt::new(HKDF_SHA256, salt).extract(&ikm);
    (
        expand(&prk, b"Content-Encoding: aes128gcm\0", 16),
        expand(&prk, b"Content-Encoding: nonce\0", 12),
    )
}

fn aead(cek: &[u8]) -> LessSafeKey {
    LessSafeKey::new(UnboundKey::new(&AES_128_GCM, cek).expect("a 16-byte key"))
}

fn nonce(n: &[u8]) -> Nonce {
    Nonce::try_assume_unique_for_key(n).expect("a 12-byte nonce")
}

/// `plaintext` sealed to the device whose public key and auth secret are
/// `p256dh` and `auth` (base64url, as the device handed them): the body of
/// the POST to its endpoint, `Content-Encoding: aes128gcm`.
pub fn seal(p256dh: &str, auth: &str, plaintext: &[u8]) -> Result<Vec<u8>, Error> {
    if plaintext.len() > MAX_PLAINTEXT {
        return Err(err(format!("a push carries at most {MAX_PLAINTEXT} bytes, not {}", plaintext.len())));
    }
    let ua_public = B64.decode(p256dh.trim_end_matches('=')).map_err(|e| err(format!("p256dh: {e}")))?;
    let auth = B64.decode(auth.trim_end_matches('=')).map_err(|e| err(format!("auth: {e}")))?;
    let ua = PublicKey::from_sec1_bytes(&ua_public).map_err(|_| err("p256dh is not a P-256 point"))?;
    let ephemeral = new_secret();
    let as_public = uncompressed(&ephemeral.public_key());
    let shared = p256::ecdh::diffie_hellman(ephemeral.to_nonzero_scalar(), ua.as_affine());
    let salt: [u8; 16] = random();
    let (cek, n) = content_keys(shared.raw_secret_bytes(), &auth, &ua_public, &as_public, &salt);
    // One record, the last: the delimiter 0x02, no padding.
    let mut record = plaintext.to_vec();
    record.push(2);
    aead(&cek)
        .seal_in_place_append_tag(nonce(&n), Aad::empty(), &mut record)
        .map_err(|_| err("sealing failed"))?;
    let mut body = Vec::with_capacity(86 + record.len());
    body.extend_from_slice(&salt);
    body.extend_from_slice(&RECORD.to_be_bytes());
    body.push(as_public.len() as u8);
    body.extend_from_slice(&as_public);
    body.extend_from_slice(&record);
    Ok(body)
}

/// A message's body opened with the device's `keys`: what was sealed.
pub fn open(keys: &Keys, body: &[u8]) -> Result<Vec<u8>, Error> {
    if body.len() < 21 {
        return Err(err("too short for a Web Push message"));
    }
    let (salt, rest) = body.split_at(16);
    let rs = u32::from_be_bytes(rest[..4].try_into().unwrap_or_default());
    let idlen = rest[4] as usize;
    let rest = &rest[5..];
    if rest.len() < idlen {
        return Err(err("the sender's key is cut short"));
    }
    let (as_public, record) = rest.split_at(idlen);
    if record.len() > rs as usize {
        return Err(err("more than one record; a push is one"));
    }
    let sender = PublicKey::from_sec1_bytes(as_public).map_err(|_| err("the sender's key is not a P-256 point"))?;
    let ua_public = uncompressed(&keys.secret.public_key());
    let shared = p256::ecdh::diffie_hellman(keys.secret.to_nonzero_scalar(), sender.as_affine());
    let (cek, n) = content_keys(shared.raw_secret_bytes(), &keys.auth, &ua_public, as_public, salt);
    let mut buf = record.to_vec();
    let plain = aead(&cek)
        .open_in_place(nonce(&n), Aad::empty(), &mut buf)
        .map_err(|_| err("it does not open with this device's keys"))?;
    // The padding is zeros before the delimiter, from the end.
    let end = plain.iter().rposition(|b| *b != 0).ok_or_else(|| err("no delimiter"))?;
    match plain[end] {
        2 => Ok(plain[..end].to_vec()),
        _ => Err(err("not the last record's delimiter")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_message_opens_with_the_devices_keys_and_no_others() {
        let keys = Keys::generate();
        let body = seal(&keys.p256dh(), &keys.auth(), b"{\"title\":\"3 to review\"}").unwrap();
        assert_eq!(open(&keys, &body).unwrap(), b"{\"title\":\"3 to review\"}");
        // The header: the salt, the record size, the sender's key (65).
        assert_eq!(&body[16..20], &4096u32.to_be_bytes());
        assert_eq!(body[20], 65);
        assert!(open(&Keys::generate(), &body).is_err());
        // Kept and read back, the keys are the same.
        let again = Keys::from_private(&keys.to_private()).unwrap();
        assert_eq!(open(&again, &body).unwrap(), b"{\"title\":\"3 to review\"}");
        assert_eq!(again.p256dh(), keys.p256dh());
    }

    /// RFC 8291 §5's example, opened: the device's keys and the message as
    /// the RFC gives them. Falsified by swapping the two keys in the info.
    #[test]
    fn the_rfcs_example_opens() {
        let ua_private = "q1dXpw3UpT5VOmu_cf_v6ih07Aems3njxI-JWgLcM94";
        let auth = "BTBZMqHH6r4Tts7J_aSIgg";
        let body = "DGv6ra1nlYgDCS1FRnbzlwAAEABBBP4z9KsN6nGRTbVYI_c7VJSPQTBtkgcy27mlmlMoZIIgDll6e3vCYLocInmYWAmS6TlzAC8wEqKK6PBru3jl7A_yl95bQpu6cVPTpK4Mqgkf1CXztLVBSt2Ks3oZwbuwXPXLWyouBWLVWGNWQexSgSxsj_Qulcy4a-fN";
        let k = B64.decode(ua_private).unwrap();
        let keys = Keys {
            secret: SecretKey::from_slice(&k).unwrap(),
            auth: B64.decode(auth).unwrap().try_into().unwrap(),
        };
        assert_eq!(
            keys.p256dh(),
            "BCVxsr7N_eNgVRqvHtD0zTZsEc6-VV-JvLexhqUzORcxaOzi6-AYWXvTBHm4bjyPjs7Vd8pZGH6SRpkNtoIAiw4"
        );
        assert_eq!(
            open(&keys, &B64.decode(body).unwrap()).unwrap(),
            b"When I grow up, I want to be a watermelon"
        );
    }

    #[test]
    fn too_much_is_refused() {
        let keys = Keys::generate();
        assert!(seal(&keys.p256dh(), &keys.auth(), &vec![b'a'; MAX_PLAINTEXT + 1]).is_err());
        assert!(seal(&keys.p256dh(), &keys.auth(), &vec![b'a'; MAX_PLAINTEXT]).is_ok());
    }
}
