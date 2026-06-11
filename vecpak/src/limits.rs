use crate::error::{Error, Result};

pub const DEFAULT_MAX_DEPTH: usize = 16;
pub const DEFAULT_MAX_CONTAINER_LEN: usize = 16_777_216;

pub(crate) const PREALLOC_CAP: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_depth: usize,
    pub max_container_len: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            max_depth: DEFAULT_MAX_DEPTH,
            max_container_len: DEFAULT_MAX_CONTAINER_LEN,
        }
    }
}

impl Limits {
    pub const KEYS: [&'static str; 2] = ["max_depth", "max_container_len"];

    pub fn unlimited() -> Self {
        Limits {
            max_depth: usize::MAX,
            max_container_len: usize::MAX,
        }
    }

    pub fn from_overrides<I, S>(overrides: I) -> Result<Self>
    where
        I: IntoIterator<Item = (S, usize)>,
        S: AsRef<str>,
    {
        let mut limits = Limits::default();
        for (key, value) in overrides {
            limits.set(key.as_ref(), value)?;
        }
        Ok(limits)
    }

    pub fn set(&mut self, key: &str, value: usize) -> Result<()> {
        match key {
            "max_depth" => self.max_depth = value,
            "max_container_len" => self.max_container_len = value,
            other => {
                return Err(Error::Message(format!("unknown limit option: {other}")));
            }
        }
        Ok(())
    }
}

#[derive(Debug)]
pub(crate) struct Budget<'a> {
    pub limits: &'a Limits,
    pub depth: usize,
}

impl<'a> Budget<'a> {
    pub fn new(limits: &'a Limits) -> Self {
        Budget { limits, depth: 0 }
    }

    #[inline]
    pub fn enter(&mut self) -> std::result::Result<(), &'static str> {
        self.depth += 1;
        if self.depth > self.limits.max_depth {
            return Err("depth_limit_exceeded");
        }
        Ok(())
    }

    #[inline]
    pub fn leave(&mut self) {
        self.depth -= 1;
    }

    #[inline]
    pub fn account_container(
        &self,
        count: usize,
        remaining: usize,
    ) -> std::result::Result<usize, &'static str> {
        if count > self.limits.max_container_len {
            return Err("container_too_large");
        }
        if count > remaining {
            return Err("count_exceeds_input");
        }
        Ok(count.min(PREALLOC_CAP))
    }
}
