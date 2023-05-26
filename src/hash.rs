use md5;
use sha1;
use sha1::Digest;
use sha2;

pub enum Hasher {
    Md5(md5::Context),
    Sha1(sha1::Sha1),
    Sha224(sha2::Sha224),
    Sha256(sha2::Sha256),
    Sha384(sha2::Sha384),
    Sha512(sha2::Sha512),
}

impl Hasher {
    pub fn new(algo: &str) -> Option<Self> {
        match algo {
            "md5" => Some(Hasher::Md5(md5::Context::new())),
            "sha1" => Some(Hasher::Sha1(sha1::Sha1::new())),
            "sha224" => Some(Hasher::Sha224(sha2::Sha224::new())),
            "sha256" => Some(Hasher::Sha256(sha2::Sha256::new())),
            "sha384" => Some(Hasher::Sha384(sha2::Sha384::new())),
            "sha512" => Some(Hasher::Sha512(sha2::Sha512::new())),
            _ => None,
        }
    }

    pub fn update(&mut self, data: &[u8]) {
        match self {
            Hasher::Md5(h) => h.consume(data),
            Hasher::Sha1(h) => h.update(data),
            Hasher::Sha224(h) => h.update(data),
            Hasher::Sha256(h) => h.update(data),
            Hasher::Sha384(h) => h.update(data),
            Hasher::Sha512(h) => h.update(data),
        }
    }

    pub fn hexdigest(self) -> String {
        match self {
            Hasher::Md5(h) => format!("{:x}", h.compute()),
            Hasher::Sha1(h) => format!("{:x}", h.finalize()),
            Hasher::Sha224(h) => format!("{:x}", h.finalize()),
            Hasher::Sha256(h) => format!("{:x}", h.finalize()),
            Hasher::Sha384(h) => format!("{:x}", h.finalize()),
            Hasher::Sha512(h) => format!("{:x}", h.finalize()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_md5() {
        let mut hasher = Hasher::new("md5").unwrap();
        hasher.update(b"hello");
        assert_eq!(hasher.hexdigest(), "5d41402abc4b2a76b9719d911017c592");
    }

    #[test]
    fn test_sha1() {
        let mut hasher = Hasher::new("sha1").unwrap();
        hasher.update(b"hello");
        assert_eq!(
            hasher.hexdigest(),
            "aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"
        );
    }

    #[test]
    fn test_sha224() {
        let mut hasher = Hasher::new("sha224").unwrap();
        hasher.update(b"hello");
        assert_eq!(
            hasher.hexdigest(),
            "ea09ae9cc6768c50fcee903ed054556e5bfc8347907f12598aa24193"
        );
    }

    #[test]
    fn test_sha256() {
        let mut hasher = Hasher::new("sha256").unwrap();
        hasher.update(b"hello");
        assert_eq!(
            hasher.hexdigest(),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha384() {
        let mut hasher = Hasher::new("sha384").unwrap();
        hasher.update(b"hello");
        assert_eq!(hasher.hexdigest(), "59e1748777448c69de6b800d7a33bbfb9ff1b463e44354c3553bcdb9c666fa90125a3c79f90397bdf5f6a13de828684f");
    }

    #[test]
    fn test_sha512() {
        let mut hasher = Hasher::new("sha512").unwrap();
        hasher.update(b"hello");
        assert_eq!(hasher.hexdigest(), "9b71d224bd62f3785d96d46ad3ea3d73319bfbc2890caadae2dff72519673ca72323c3d99ba5c11d7c7acc6e14b8c5da0c4663475c2e5c3adef46f73bcdec043");
    }

    #[test]
    fn test_invalid_algo() {
        let hasher = Hasher::new("invalid");
        assert!(hasher.is_none());
    }
}
