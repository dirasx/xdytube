use anyhow::{Context as _, Result};
use serde::{Serialize, de::DeserializeOwned};

pub fn load<T: DeserializeOwned>(key: &'static str) -> Option<T> {
    match backend::read(key) {
        Ok(Some(data)) => serde_json::from_slice(&data).ok(),
        Ok(None) => None,
        Err(_) => {
            eprintln!("Could not read the saved YouTube sign-in from Keychain");
            None
        }
    }
}

pub fn save<T: Serialize>(key: &'static str, value: Option<&T>) -> Result<()> {
    let data = value.map(serde_json::to_vec).transpose()?;
    backend::write(key, data.as_deref()).context("Could not update the saved YouTube sign-in in Keychain")
}

#[cfg(all(target_os = "macos", not(test)))]
mod backend {
    use anyhow::Result;
    use security_framework::passwords::{delete_generic_password, get_generic_password, set_generic_password};
    use sha2::{Digest, Sha256};

    const SERVICE: &str = "io.github.dirasx.xdytube";
    const ITEM_NOT_FOUND: i32 = -25300;

    fn account(key: &str) -> String {
        let profile = crate::store::data_dir();
        let profile = profile.canonicalize().unwrap_or(profile);
        let namespace = Sha256::digest(profile.as_os_str().as_encoded_bytes());
        use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
        format!("{key}:{}", URL_SAFE_NO_PAD.encode(namespace))
    }

    pub fn read(key: &str) -> Result<Option<Vec<u8>>> {
        match get_generic_password(SERVICE, &account(key)) {
            Ok(data) => Ok(Some(data)),
            Err(error) if error.code() == ITEM_NOT_FOUND => Ok(None),
            Err(error) => Err(error.into()),
        }
    }

    pub fn write(key: &str, data: Option<&[u8]>) -> Result<()> {
        let account = account(key);
        if let Some(data) = data {
            set_generic_password(SERVICE, &account, data)?;
        } else if let Err(error) = delete_generic_password(SERVICE, &account)
            && error.code() != ITEM_NOT_FOUND
        {
            return Err(error.into());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{load, save};

    #[test]
    fn signing_out_removes_only_the_selected_credential() {
        save("test-oauth", Some(&"fixture-oauth")).unwrap();
        save("test-history", Some(&"fixture-history")).unwrap();
        save::<String>("test-oauth", None).unwrap();
        assert_eq!(load::<String>("test-oauth"), None);
        assert_eq!(load::<String>("test-history").as_deref(), Some("fixture-history"));
    }
}

#[cfg(test)]
mod backend {
    use anyhow::Result;
    use std::{collections::HashMap, sync::{LazyLock, Mutex}};

    static ITEMS: LazyLock<Mutex<HashMap<&'static str, Vec<u8>>>> = LazyLock::new(Mutex::default);

    pub fn read(key: &'static str) -> Result<Option<Vec<u8>>> {
        Ok(ITEMS.lock().unwrap().get(key).cloned())
    }

    pub fn write(key: &'static str, data: Option<&[u8]>) -> Result<()> {
        let mut items = ITEMS.lock().unwrap();
        if let Some(data) = data {
            items.insert(key, data.to_vec());
        } else {
            items.remove(key);
        }
        Ok(())
    }
}
