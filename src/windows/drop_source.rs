use windows::Win32::Foundation::{
    DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, S_OK,
};
use windows::Win32::System::Ole::{DROPEFFECT, IDropSource, IDropSource_Impl};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::core::{BOOL, HRESULT, implement};

#[implement(IDropSource)]
pub(crate) struct DropSource;

#[allow(non_snake_case)]
impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape_pressed: BOOL, key_state: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape_pressed.as_bool() {
            return DRAGDROP_S_CANCEL;
        }

        if !key_state.contains(MK_LBUTTON) {
            return DRAGDROP_S_DROP;
        }

        S_OK
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_drop_source() {
        let _source: IDropSource = DropSource.into();
    }
}
