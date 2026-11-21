//! Files dragged onto Pillbox (OLE drop target) and dragged out of the shelf into other
//! apps (OLE drag source with the shell's own data object, so Explorer, mail, chat apps and
//! browsers all accept it).

use std::{cell::RefCell, path::PathBuf};

use anyhow::{Result, anyhow};
use windows::{
    Win32::{
        Foundation::{
            DRAGDROP_S_CANCEL, DRAGDROP_S_DROP, DRAGDROP_S_USEDEFAULTCURSORS, HWND, POINTL, S_OK,
        },
        System::{
            Com::{CoTaskMemFree, DVASPECT_CONTENT, FORMATETC, IDataObject, TYMED_HGLOBAL},
            Ole::{
                CF_HDROP, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_LINK, DROPEFFECT_MOVE,
                DROPEFFECT_NONE, DoDragDrop, IDropSource, IDropSource_Impl, IDropTarget,
                IDropTarget_Impl, RegisterDragDrop, ReleaseStgMedium, RevokeDragDrop,
            },
            SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS},
        },
        UI::Shell::{
            BHID_DataObject, Common::ITEMIDLIST, DragQueryFileW, HDROP, IShellItemArray,
            SHCreateShellItemArrayFromIDLists, SHParseDisplayName,
        },
    },
    core::{BOOL, HRESULT, HSTRING, Ref, implement},
};

use super::window::{Event, push_event};

thread_local! {
    static DROPPED: RefCell<Vec<PathBuf>> = const { RefCell::new(Vec::new()) };
}

/// Files from the last drop; the app collects them after `Event::FilesDropped`.
pub fn take_dropped() -> Vec<PathBuf> {
    DROPPED.with_borrow_mut(std::mem::take)
}

fn has_files(data: &IDataObject) -> bool {
    let fmt = hdrop_format();
    // SAFETY: a valid FORMATETC describing CF_HDROP in global memory.
    (unsafe { data.QueryGetData(&fmt) }) == S_OK
}

fn hdrop_format() -> FORMATETC {
    FORMATETC {
        cfFormat: CF_HDROP.0,
        ptd: std::ptr::null_mut(),
        dwAspect: DVASPECT_CONTENT.0,
        lindex: -1,
        tymed: TYMED_HGLOBAL.0 as u32,
    }
}

fn files_of(data: &IDataObject) -> Vec<PathBuf> {
    let fmt = hdrop_format();
    let mut out = Vec::new();
    // SAFETY: GetData returns a storage medium we own and release once; the HDROP inside is
    // valid until then. Names are read into buffers sized from the reported lengths.
    unsafe {
        let Ok(mut medium) = data.GetData(&fmt) else {
            return out;
        };
        let drop = HDROP(medium.u.hGlobal.0);
        let count = DragQueryFileW(drop, u32::MAX, None);
        for i in 0..count {
            let len = DragQueryFileW(drop, i, None) as usize;
            let mut buf = vec![0u16; len + 1];
            let got = DragQueryFileW(drop, i, Some(&mut buf)) as usize;
            out.push(PathBuf::from(String::from_utf16_lossy(
                &buf[..got.min(len)],
            )));
        }
        ReleaseStgMedium(&mut medium);
    }
    out
}

#[implement(IDropTarget)]
struct DropTarget;

impl IDropTarget_Impl for DropTarget_Impl {
    fn DragEnter(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let ok = data.as_ref().is_some_and(has_files);
        if !effect.is_null() {
            // SAFETY: OLE passes a valid in/out pointer.
            unsafe { *effect = if ok { DROPEFFECT_COPY } else { DROPEFFECT_NONE } };
        }
        if ok {
            push_event(Event::DragEnter);
        }
        Ok(())
    }

    fn DragOver(
        &self,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        if !effect.is_null() {
            // SAFETY: OLE passes a valid in/out pointer.
            unsafe { *effect = DROPEFFECT_COPY };
        }
        Ok(())
    }

    fn DragLeave(&self) -> windows::core::Result<()> {
        push_event(Event::DragLeave);
        Ok(())
    }

    fn Drop(
        &self,
        data: Ref<'_, IDataObject>,
        _keys: MODIFIERKEYS_FLAGS,
        _pt: &POINTL,
        effect: *mut DROPEFFECT,
    ) -> windows::core::Result<()> {
        let files = data.as_ref().map(files_of).unwrap_or_default();
        if !effect.is_null() {
            // SAFETY: OLE passes a valid in/out pointer. We keep references, never move.
            unsafe {
                *effect = if files.is_empty() {
                    DROPEFFECT_NONE
                } else {
                    DROPEFFECT_COPY
                }
            };
        }
        if !files.is_empty() {
            DROPPED.with_borrow_mut(|d| d.extend(files));
            push_event(Event::FilesDropped);
        } else {
            push_event(Event::DragLeave);
        }
        Ok(())
    }
}

/// Keeps Pillbox registered as a drop target; revoked on drop.
pub struct DropRegistration(HWND);

impl DropRegistration {
    pub fn register(hwnd: HWND) -> Result<Self> {
        let target: IDropTarget = DropTarget.into();
        // SAFETY: OLE is initialised on this (UI) thread; OLE keeps its own reference.
        unsafe { RegisterDragDrop(hwnd, &target) }.map_err(|e| anyhow!("RegisterDragDrop: {e}"))?;
        Ok(Self(hwnd))
    }
}

impl Drop for DropRegistration {
    fn drop(&mut self) {
        // SAFETY: paired with the successful RegisterDragDrop.
        let _ = unsafe { RevokeDragDrop(self.0) };
    }
}

#[implement(IDropSource)]
struct DropSource;

impl IDropSource_Impl for DropSource_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        if escape.as_bool() {
            DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 {
            DRAGDROP_S_DROP
        } else {
            S_OK
        }
    }

    fn GiveFeedback(&self, _effect: DROPEFFECT) -> HRESULT {
        DRAGDROP_S_USEDEFAULTCURSORS
    }
}

/// Owned shell item id lists, freed on drop.
struct Pidls(Vec<*mut ITEMIDLIST>);

impl Drop for Pidls {
    fn drop(&mut self) {
        for p in &self.0 {
            // SAFETY: each pointer came from SHParseDisplayName and is freed exactly once.
            unsafe { CoTaskMemFree(Some((*p).cast_const().cast())) };
        }
    }
}

/// Drag these files out of Pillbox. Blocks in OLE's own loop until the drop happens or is
/// cancelled. Returns whether something accepted the drop.
pub fn drag_out(files: &[PathBuf]) -> Result<bool> {
    let mut pidls = Pidls(Vec::new());
    for f in files {
        let name = HSTRING::from(f.as_os_str());
        let mut pidl: *mut ITEMIDLIST = std::ptr::null_mut();
        // SAFETY: valid string and out-pointer; the returned id list is owned by `pidls`.
        if unsafe { SHParseDisplayName(&name, None, &mut pidl, 0, None) }.is_ok() && !pidl.is_null()
        {
            pidls.0.push(pidl);
        }
    }
    if pidls.0.is_empty() {
        return Err(anyhow!("none of the files exist any more"));
    }
    let ptrs: Vec<*const ITEMIDLIST> = pidls.0.iter().map(|p| p.cast_const()).collect();
    // SAFETY: the id lists stay alive (owned by `pidls`) until after DoDragDrop returns.
    unsafe {
        let items: IShellItemArray = SHCreateShellItemArrayFromIDLists(&ptrs)?;
        let data: IDataObject = items.BindToHandler(None, &BHID_DataObject)?;
        let source: IDropSource = DropSource.into();
        let mut effect = DROPEFFECT_NONE;
        let hr = DoDragDrop(
            &data,
            &source,
            DROPEFFECT_COPY | DROPEFFECT_LINK | DROPEFFECT_MOVE,
            &mut effect,
        );
        Ok(hr == DRAGDROP_S_DROP && effect != DROPEFFECT_NONE)
    }
}
