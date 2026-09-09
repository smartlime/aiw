use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::error::Error;
use std::fmt;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[serde(deny_unknown_fields)]
pub struct KeychainCoordinates {
    pub service: String,
    pub account: String,
}

impl KeychainCoordinates {
    pub fn new(service: impl Into<String>, account: impl Into<String>) -> Self {
        Self {
            service: service.into(),
            account: account.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SecretStoreError(String);

impl SecretStoreError {
    pub fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }

    fn backend(operation: &str, code: i32) -> Self {
        Self(format!("Keychain {operation} failed with status {code}"))
    }

    #[cfg(not(target_os = "macos"))]
    fn unsupported() -> Self {
        Self("macOS Keychain is unavailable on this platform".into())
    }
}

impl fmt::Display for SecretStoreError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for SecretStoreError {}

pub trait SecretStore {
    fn get(&self, coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError>;
    fn set(&self, coordinates: &KeychainCoordinates, value: &[u8]) -> Result<(), SecretStoreError>;
    fn delete(&self, coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError>;
}

#[derive(Default)]
pub struct MacOSKeychainAdapter;

#[cfg(target_os = "macos")]
impl SecretStore for MacOSKeychainAdapter {
    fn get(&self, coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError> {
        use security_framework::passwords::get_generic_password;
        use security_framework_sys::base::errSecItemNotFound;

        match get_generic_password(&coordinates.service, &coordinates.account) {
            Ok(value) => Ok(Some(value)),
            Err(error) if error.code() == errSecItemNotFound => Ok(None),
            Err(error) => Err(SecretStoreError::backend("read", error.code())),
        }
    }

    fn set(&self, coordinates: &KeychainCoordinates, value: &[u8]) -> Result<(), SecretStoreError> {
        security_framework::passwords::set_generic_password(
            &coordinates.service,
            &coordinates.account,
            value,
        )
        .map_err(|error| SecretStoreError::backend("write", error.code()))
    }

    fn delete(&self, coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError> {
        use security_framework::passwords::delete_generic_password;
        use security_framework_sys::base::errSecItemNotFound;

        match delete_generic_password(&coordinates.service, &coordinates.account) {
            Ok(()) => Ok(()),
            Err(error) if error.code() == errSecItemNotFound => Ok(()),
            Err(error) => Err(SecretStoreError::backend("delete", error.code())),
        }
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for MacOSKeychainAdapter {
    fn get(&self, _coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }

    fn set(
        &self,
        _coordinates: &KeychainCoordinates,
        _value: &[u8],
    ) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }

    fn delete(&self, _coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError> {
        Err(SecretStoreError::unsupported())
    }
}

#[derive(Default)]
pub struct MemorySecretStore {
    values: RefCell<BTreeMap<KeychainCoordinates, Vec<u8>>>,
    reads: Cell<usize>,
}

impl MemorySecretStore {
    pub fn from_values(values: BTreeMap<KeychainCoordinates, String>) -> Self {
        Self {
            values: RefCell::new(
                values
                    .into_iter()
                    .map(|(coordinates, value)| (coordinates, value.into_bytes()))
                    .collect(),
            ),
            reads: Cell::new(0),
        }
    }

    pub fn read_count(&self) -> usize {
        self.reads.get()
    }
}

impl SecretStore for MemorySecretStore {
    fn get(&self, coordinates: &KeychainCoordinates) -> Result<Option<Vec<u8>>, SecretStoreError> {
        self.reads.set(self.reads.get() + 1);
        Ok(self.values.borrow().get(coordinates).cloned())
    }

    fn set(&self, coordinates: &KeychainCoordinates, value: &[u8]) -> Result<(), SecretStoreError> {
        self.values
            .borrow_mut()
            .insert(coordinates.clone(), value.to_owned());
        Ok(())
    }

    fn delete(&self, coordinates: &KeychainCoordinates) -> Result<(), SecretStoreError> {
        self.values.borrow_mut().remove(coordinates);
        Ok(())
    }
}
