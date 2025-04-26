use crate::util::NOTE_TO_DELETE;
use block::ConcreteBlock;
use cocoa::appkit::{NSEvent, NSEventType, NSMenu, NSMenuItem};
use cocoa::base::{NO, YES, id, nil, selector};
use cocoa::foundation::{NSPoint, NSRect, NSString};
use core_foundation::base::TCFType;
use core_foundation::string::{CFString, CFStringRef};
use objc::runtime::{Class, Object};
use objc::{class, msg_send, sel, sel_impl};
use std::os::raw::c_void;
use std::sync::Arc;
use std::sync::Mutex;
use std::thread;
use uuid::Uuid;

pub struct ContextMenu {
    menu: id,
    direct_delete_callback: Option<DirectDeleteCallback>,
}

pub enum MenuAction {
    Delete(Uuid),
}

pub type MenuCallback = Box<dyn Fn(MenuAction) + Send + 'static>;

pub type DirectDeleteCallback = Box<dyn Fn(Uuid) -> bool + Send + 'static>;

impl ContextMenu {
    pub fn new() -> Self {
        unsafe {
            let menu: id = msg_send![class!(NSMenu), new];
            let _: () = msg_send![menu, setAutoenablesItems:NO];

            Self {
                menu,
                direct_delete_callback: None,
            }
        }
    }

    pub fn add_delete_item(&mut self, title: &str, note_id: Uuid) -> &mut Self {
        unsafe {
            let title_ns = NSString::alloc(nil).init_str(title);
            let menu_item: id = msg_send![class!(NSMenuItem), alloc];
            let menu_item: id = msg_send![menu_item, initWithTitle:title_ns action:selector("menuItemClicked:") keyEquivalent:NSString::alloc(nil).init_str("")];

            let _: () = msg_send![menu_item, setTag:1];

            let note_id_str = note_id.to_string();
            let note_id_ns = NSString::alloc(nil).init_str(&note_id_str);
            let _: () = msg_send![menu_item, setRepresentedObject:note_id_ns];

            let _: () = msg_send![menu_item, setEnabled:YES];
            let _: () = msg_send![self.menu, addItem:menu_item];
        }
        self
    }

    pub fn show_at_position(&self, x: f64, y: f64, callback: MenuCallback) {
        unsafe {
            let cls = define_menu_handler_class(&callback, &self.direct_delete_callback);
            let handler: id = msg_send![cls, alloc];
            let handler: id = msg_send![handler, init];

            let items_count: usize = msg_send![self.menu, numberOfItems];
            for i in 0..items_count {
                let item: id = msg_send![self.menu, itemAtIndex:i];
                let _: () = msg_send![item, setTarget:handler];
            }

            let app: id = msg_send![class!(NSApplication), sharedApplication];

            let mouse_location: NSPoint = msg_send![class!(NSEvent), mouseLocation];

            let menu = self.menu;

            let dispatch_queue = class!(NSOperationQueue);
            let main_queue: id = msg_send![dispatch_queue, mainQueue];

            let block = ConcreteBlock::new(move || {
                let current_event: id = msg_send![app, currentEvent];

                let nil_id: id = nil;
                let _: () = msg_send![
                    menu,
                    popUpMenuPositioningItem:nil_id
                    atLocation:mouse_location
                    inView:nil_id
                ];
            })
            .copy();

            let _: () = msg_send![main_queue, addOperationWithBlock:block];
        }
    }

    pub fn set_direct_delete_callback<F>(&mut self, callback: F) -> &mut Self
    where
        F: Fn(Uuid) -> bool + Send + 'static,
    {
        self.direct_delete_callback = Some(Box::new(callback));
        self
    }
}

fn define_menu_handler_class(
    callback: &MenuCallback,
    direct_delete_callback: &Option<DirectDeleteCallback>,
) -> *const Class {
    use std::sync::Once;
    static mut DELEGATE_CLASS: *const Class = 0 as *const Class;
    static INIT: Once = Once::new();

    INIT.call_once(|| unsafe {
        let superclass = class!(NSObject);
        let mut decl = objc::declare::ClassDecl::new("RustMenuHandler", superclass).unwrap();

        decl.add_ivar::<*mut c_void>("callback");
        decl.add_ivar::<*mut c_void>("directDeleteCallback");

        extern "C" fn menu_item_clicked(this: &Object, _: objc::runtime::Sel, sender: id) {
            unsafe {
                println!("Menu item clicked!");
                let tag: i64 = msg_send![sender, tag];
                println!("Menu item tag: {}", tag);
                if tag != 1 {
                    println!("Not a delete action, tag is {}", tag);
                    return;
                }

                let note_id_obj: id = msg_send![sender, representedObject];
                if note_id_obj == nil {
                    println!("note_id_obj is nil");
                    return;
                }

                let ns_string: id = note_id_obj;
                let note_id_cstr: *const std::os::raw::c_char = msg_send![ns_string, UTF8String];
                let note_id_rust = std::ffi::CStr::from_ptr(note_id_cstr)
                    .to_str()
                    .unwrap_or("");

                println!("Note ID from menu: {}", note_id_rust);

                match Uuid::parse_str(note_id_rust) {
                    Ok(note_id) => {
                        println!("Successfully parsed UUID: {}", note_id);

                        let direct_callback_ptr: *mut c_void =
                            *this.get_ivar("directDeleteCallback");
                        if !direct_callback_ptr.is_null() {
                            println!("Found direct delete callback, trying it first");
                            let direct_callback =
                                &*(direct_callback_ptr as *const DirectDeleteCallback);

                            if direct_callback(note_id) {
                                println!("Direct deletion succeeded!");
                                return;
                            } else {
                                println!("Direct deletion failed, trying alternative methods");
                            }
                        } else {
                            println!("No direct delete callback available");
                        }

                        println!("Setting note {} for direct deletion", note_id);
                        if let Ok(mut guard) = NOTE_TO_DELETE.lock() {
                            *guard = Some(note_id);
                            println!("NOTE FOR DELETION SET DIRECTLY: {}", note_id);

                            drop(guard);

                            for i in 0..5 {
                                if i > 0 {
                                    std::thread::sleep(std::time::Duration::from_millis(
                                        50 * i as u64,
                                    ));
                                }

                                println!("Forcing UI refresh attempt {}", i + 1);
                                let app: id = msg_send![class!(NSApplication), sharedApplication];
                                let _: () = msg_send![app, updateWindows];

                                if let Ok(check_guard) = NOTE_TO_DELETE.lock() {
                                    if check_guard.is_none() {
                                        println!("Deletion was processed on attempt {}", i + 1);
                                        break;
                                    }
                                }
                            }
                        } else {
                            println!("Failed to lock NOTE_TO_DELETE mutex for direct deletion");
                        }

                        let callback_ptr: *mut c_void = *this.get_ivar("callback");

                        if callback_ptr.is_null() {
                            println!("ERROR: callback_ptr is null!");
                            return;
                        }

                        let callback = &*(callback_ptr as *const MenuCallback);
                        println!("Calling callback for delete action");
                        callback(MenuAction::Delete(note_id));
                        println!("Callback completed");
                    }
                    Err(e) => {
                        println!("Failed to parse UUID: {}", e);
                    }
                }
            }
        }

        decl.add_method(
            sel!(menuItemClicked:),
            menu_item_clicked as extern "C" fn(&Object, objc::runtime::Sel, id),
        );

        extern "C" fn init_with_callback(this: &mut Object, _: objc::runtime::Sel) -> id {
            unsafe {
                let this_ptr: id = msg_send![super(this, class!(NSObject)), init];
                if this_ptr != nil {
                    let callback_box = Box::new(Box::new(|_: MenuAction| {}) as MenuCallback);
                    let callback_ptr = Box::into_raw(callback_box) as *mut c_void;
                    this.set_ivar("callback", callback_ptr);

                    this.set_ivar("directDeleteCallback", std::ptr::null_mut() as *mut c_void);
                }
                this_ptr
            }
        }

        decl.add_method(
            sel!(init),
            init_with_callback as extern "C" fn(&mut Object, objc::runtime::Sel) -> id,
        );

        extern "C" fn dealloc(this: &mut Object, _: objc::runtime::Sel) {
            unsafe {
                let callback_ptr: *mut c_void = *this.get_ivar("callback");
                if !callback_ptr.is_null() {
                    let _ = Box::from_raw(callback_ptr as *mut MenuCallback);
                }

                let direct_callback_ptr: *mut c_void = *this.get_ivar("directDeleteCallback");
                if !direct_callback_ptr.is_null() {
                    let _ = Box::from_raw(direct_callback_ptr as *mut DirectDeleteCallback);
                }

                let _: () = msg_send![super(this, class!(NSObject)), dealloc];
            }
        }

        decl.add_method(
            sel!(dealloc),
            dealloc as extern "C" fn(&mut Object, objc::runtime::Sel),
        );

        DELEGATE_CLASS = decl.register();
    });

    unsafe {
        let cls = DELEGATE_CLASS;
        let handler: id = msg_send![cls, alloc];
        let handler: id = msg_send![handler, init];
        let handler_obj = &mut *(handler as *mut Object);

        let callback_ptr_ivar: *mut c_void = *handler_obj.get_ivar("callback");
        if !callback_ptr_ivar.is_null() {
            let _old_callback = Box::from_raw(callback_ptr_ivar as *mut MenuCallback);
        }

        let new_callback_box = Box::new(callback.clone());
        let new_callback_ptr = Box::into_raw(new_callback_box) as *mut c_void;
        handler_obj.set_ivar("callback", new_callback_ptr);

        if let Some(direct_callback) = direct_delete_callback {
            let direct_callback_ptr_ivar: *mut c_void =
                *handler_obj.get_ivar("directDeleteCallback");
            if !direct_callback_ptr_ivar.is_null() {
                let _old_direct_callback =
                    Box::from_raw(direct_callback_ptr_ivar as *mut DirectDeleteCallback);
            }

            let new_direct_callback_box = Box::new(direct_callback.clone());
            let new_direct_callback_ptr = Box::into_raw(new_direct_callback_box) as *mut c_void;
            handler_obj.set_ivar("directDeleteCallback", new_direct_callback_ptr);
        }

        cls
    }
}

#[allow(dead_code)]
fn shared_application() -> id {
    unsafe { msg_send![class!(NSApplication), sharedApplication] }
}
