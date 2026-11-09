use std::{sync::mpsc, time::Duration};

use block2::RcBlock;
use objc2::runtime::Bool;
use objc2_foundation::{NSError, NSString};
use objc2_local_authentication::{LAContext, LAPolicy};
use security_framework::passwords;
use sha2::{Digest, Sha256};
use zeroize::Zeroizing;

use crate::vault::Key;

const KEYCHAIN_SERVICE: &str = "Chatly";
const KEYCHAIN_ACCOUNT: &str = "touch-id-unlock-key";
const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthenticationWithBiometrics;

pub enum UnlockError {
    Prompt(String),
    Invalidated,
}

pub fn available() -> bool {
    let ctx = unsafe { LAContext::new() };
    unsafe { ctx.canEvaluatePolicy_error(POLICY) }.is_ok()
}

fn authenticate(reason: &str) -> Result<[u8; 32], String> {
    let ctx = unsafe { LAContext::new() };
    unsafe { ctx.setLocalizedFallbackTitle(Some(&NSString::from_str(""))) };

    let (tx, rx) = mpsc::channel::<Result<(), String>>();
    let reply = RcBlock::new(move |ok: Bool, err: *mut NSError| {
        let outcome = if ok.as_bool() {
            Ok(())
        } else {
            Err(unsafe { err.as_ref() }
                .map(|e| e.localizedDescription().to_string())
                .unwrap_or_else(|| "Touch ID didn't work.".into()))
        };
        let _ = tx.send(outcome);
    });
    unsafe { ctx.evaluatePolicy_localizedReason_reply(POLICY, &NSString::from_str(reason), &reply) };

    rx.recv_timeout(Duration::from_secs(300))
        .map_err(|_| "Touch ID timed out.".to_string())??;

    #[allow(deprecated)]
    let state = unsafe { ctx.evaluatedPolicyDomainState() }
        .map(|d| d.to_vec())
        .unwrap_or_default();
    Ok(Sha256::digest(&state).into())
}

pub fn enroll(key: &Key) -> Result<(), String> {
    let fingers = authenticate("turn on Touch ID for Chatly")?;
    let mut blob = Zeroizing::new(Vec::with_capacity(64));
    blob.extend_from_slice(key.as_ref());
    blob.extend_from_slice(&fingers);
    passwords::set_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT, &blob)
        .map_err(|e| format!("Couldn't save to the keychain: {e}"))
}

pub fn unlock() -> Result<Key, UnlockError> {
    let fingers = authenticate("unlock your chats").map_err(UnlockError::Prompt)?;
    let blob = Zeroizing::new(
        passwords::get_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT).map_err(|_| UnlockError::Invalidated)?,
    );
    if blob.len() != 64 || blob[32..] != fingers {
        return Err(UnlockError::Invalidated);
    }
    let mut key: Key = Zeroizing::new([0; 32]);
    key.copy_from_slice(&blob[..32]);
    Ok(key)
}

pub fn forget() {
    let _ = passwords::delete_generic_password(KEYCHAIN_SERVICE, KEYCHAIN_ACCOUNT);
}
