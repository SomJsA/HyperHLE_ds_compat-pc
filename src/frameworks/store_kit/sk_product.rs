/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! StoreKit product/request stubs for the delisted Dead Space IAPs.

use crate::frameworks::foundation::{ns_string, NSInteger};
use crate::objc::{
    autorelease, id, msg, msg_class, nil, objc_classes, release, ClassExports, HostObject,
    NSZonePtr,
};
use crate::Environment;

#[derive(Default)]
struct SKProductHostObject {
    product_identifier: id,
    localized_title: id,
    localized_description: id,
    price: id,
}
impl HostObject for SKProductHostObject {}

#[derive(Default)]
struct SKProductsRequestHostObject {
    delegate: id,
    product_identifiers: id,
}
impl HostObject for SKProductsRequestHostObject {}

#[derive(Default)]
struct SKProductsResponseHostObject {
    products: id,
    invalid_product_identifiers: id,
}
impl HostObject for SKProductsResponseHostObject {}

fn fake_products_enabled() -> bool {
    std::env::var_os("TOUCHHLE_DS_FAKE_MTX").is_some()
        || std::env::var_os("TOUCHHLE_DS_FAKE_MTX_AUTO").is_some()
}

fn make_fake_product(
    env: &mut Environment,
    product_identifier: &'static str,
    title: &'static str,
    description: &'static str,
    price: f64,
) -> id {
    let product: id = msg_class![env; SKProduct alloc];
    let product: id = msg![env; product init];
    let product_identifier = ns_string::get_static_str(env, product_identifier);
    let title = ns_string::get_static_str(env, title);
    let description = ns_string::get_static_str(env, description);
    let price: id = msg_class![env; NSNumber numberWithDouble:price];
    {
        let host = env.objc.borrow_mut::<SKProductHostObject>(product);
        host.product_identifier = product_identifier;
        host.localized_title = title;
        host.localized_description = description;
        host.price = price;
    }
    autorelease(env, product)
}

fn make_fake_products(env: &mut Environment) -> id {
    let products: id = msg_class![env; NSMutableArray new];
    if fake_products_enabled() {
        // Dead Space 1.3.34's three non-consumable IAPs. The app maps the
        // product identifiers to the Settings module names attack/defense/loot.
        let product = make_fake_product(
            env,
            "com.ea.mtx.827261",
            "武器输出模块",
            "永久性提高游戏过程中对敌方造成的伤害。",
            12.0,
        );
        let _: () = msg![env; products addObject:product];
        let product = make_fake_product(
            env,
            "com.ea.mtx.827281",
            "护甲完整性模块",
            "永久性减轻游戏过程中遭受的任何伤害。",
            12.0,
        );
        let _: () = msg![env; products addObject:product];
        let product = make_fake_product(
            env,
            "com.ea.mtx.827285",
            "信用点激增",
            "启用这项升级并在游戏过程中依靠打倒敌人获取更多分数。",
            12.0,
        );
        let _: () = msg![env; products addObject:product];
    }
    products
}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation SKProduct: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKProductHostObject {
        product_identifier: nil,
        localized_title: nil,
        localized_description: nil,
        price: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (id)productIdentifier {
    env.objc
        .borrow::<SKProductHostObject>(this)
        .product_identifier
}

- (id)localizedTitle {
    env.objc
        .borrow::<SKProductHostObject>(this)
        .localized_title
}

- (id)localizedDescription {
    env.objc
        .borrow::<SKProductHostObject>(this)
        .localized_description
}

- (id)price {
    env.objc.borrow::<SKProductHostObject>(this).price
}

- (id)priceLocale {
    nil
}

- (bool)downloadable {
    false
}

- (id)downloadContentLengths {
    msg_class![env; NSArray array]
}

- (id)downloadContentVersion {
    ns_string::get_static_str(env, "")
}

- (())dealloc {
    let price = env.objc.borrow::<SKProductHostObject>(this).price;
    release(env, price);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation SKProductsResponse: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKProductsResponseHostObject {
        products: nil,
        invalid_product_identifiers: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)init {
    this
}

- (id)products {
    env.objc.borrow::<SKProductsResponseHostObject>(this).products
}

- (id)invalidProductIdentifiers {
    env.objc.borrow::<SKProductsResponseHostObject>(this).invalid_product_identifiers
}

- (())dealloc {
    let products = env.objc.borrow::<SKProductsResponseHostObject>(this).products;
    let invalid = env.objc
        .borrow::<SKProductsResponseHostObject>(this)
        .invalid_product_identifiers;
    release(env, products);
    release(env, invalid);
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

@implementation SKProductsRequest: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(SKProductsRequestHostObject {
        delegate: nil,
        product_identifiers: nil,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (id)initWithProductIdentifiers:(id)identifiers { // NSSet*
    {
        let host = env.objc.borrow_mut::<SKProductsRequestHostObject>(this);
        host.product_identifiers = identifiers;
    }
    this
}

- (())setDelegate:(id)delegate {
    let host = env.objc.borrow_mut::<SKProductsRequestHostObject>(this);
    host.delegate = delegate;
}

- (())start {
    let delegate = env.objc.borrow::<SKProductsRequestHostObject>(this).delegate;
    if delegate == nil {
        return;
    }
    let products = make_fake_products(env);
    let response: id = msg_class![env; SKProductsResponse alloc];
    let response: id = msg![env; response init];
    let invalid_product_identifiers: id = msg_class![env; NSArray array];
    {
        let host = env.objc.borrow_mut::<SKProductsResponseHostObject>(response);
        host.products = products;
        host.invalid_product_identifiers = invalid_product_identifiers;
    }
    let sel = env.objc.register_host_selector(
        "productsRequest:didReceiveResponse:".to_string(),
        &mut env.mem,
    );
    let responds: bool = msg![env; delegate respondsToSelector:sel];
    if responds {
        () = msg![env; delegate productsRequest:this didReceiveResponse:response];
    }
    autorelease(env, response);
}

- (())cancel {
}

- (())dealloc {
    env.objc.dealloc_object(this, &mut env.mem)
}

@end

};

