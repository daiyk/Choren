use choren_qi::{Client, Node, ServiceBuilder, Value};
use std::sync::{Arc, Mutex};

#[tokio::test]
async fn client_calls_server_and_receives_signals() {
    let node = Node::new();
    let store = Arc::new(Mutex::new(Vec::<(String, Value)>::new()));
    let (s1, s2) = (store.clone(), store.clone());
    let weak = Arc::downgrade(&node);
    node.register(
        ServiceBuilder::new("ALMemory")
            .method("insertData", "(sm)", "v", move |a| {
                s1.lock().unwrap().push((a[0].as_str().unwrap().into(), a[1].clone()));
                if let Some(n) = weak.upgrade() {
                    n.emit("ALMemory", "dataChanged", vec![a[0].clone(), a[1].clone()]);
                }
                Ok(Value::Void)
            })
            .method("getData", "(s)", "m", move |a| {
                let key = a[0].as_str().unwrap();
                s2.lock().unwrap().iter().rev().find(|(k, _)| k == key).map(|(_, v)| v.clone()).ok_or_else(|| format!("no key {key}"))
            })
            .method("sum", "([f])", "f", |a| Ok(Value::Float(a[0].items().iter().filter_map(Value::as_f64).sum::<f64>() as f32)))
            .signal("dataChanged", "(sm)"),
    );
    let addr = node.listen("127.0.0.1:0").await.unwrap();
    let c = Client::connect(&format!("tcp://{addr}")).await.unwrap();
    assert!(c.services().await.unwrap().contains(&"ALMemory".to_string()));
    let mut changes = c.subscribe("ALMemory", "dataChanged").await.unwrap();
    c.call("ALMemory", "insertData", vec!["k".into(), Value::Int(42)]).await.unwrap();
    assert_eq!(c.call("ALMemory", "getData", vec!["k".into()]).await.unwrap().as_i64(), Some(42));
    assert_eq!(changes.recv().await.unwrap().items()[0].as_str(), Some("k"));
    // ints convert to the declared float list
    assert_eq!(c.call("ALMemory", "sum", vec![Value::List(vec![Value::Int(1), Value::Float(2.5)])]).await.unwrap().as_f64(), Some(3.5));
    let err = c.call("ALMemory", "getData", vec!["missing".into()]).await.unwrap_err();
    assert!(err.contains("no key missing"), "{err}");
    assert!(c.call("ALMotion", "getAngles", vec![]).await.is_err());
    // in-process calls go through the same signatures
    assert_eq!(node.call("ALMemory", "getData", vec!["k".into()]).await.unwrap().as_i64(), Some(42));
}

/// What NAOqi's `PackageManager._install(oss)` does with the `qi::File` a client sends: `isOpen`,
/// `size`, `operationProgress`, then `read(offset, chunk)` until the end.
#[tokio::test]
async fn servers_call_back_objects_that_clients_send() {
    let node = Node::new();
    node.register(
        ServiceBuilder::new("PackageManager")
            .method("install", "(s)", "b", |_| Err("path overload".into()))
            .method_async("install", "(o)", "b", |a| async move {
                let Value::Object(file) = &a[0] else { return Err("object expected".into()) };
                let file = file.remote().ok_or("remote object expected")?;
                assert_eq!(file.call("isOpen", vec![]).await?, Value::Bool(true));
                let size = file.call("size", vec![]).await?.as_i64().unwrap();
                let Value::Object(progress) = file.call("operationProgress", vec![]).await? else { return Err("notifier expected".into()) };
                progress.remote().unwrap().call("notifyRunning", vec![]).await?;
                let mut bytes = vec![];
                while (bytes.len() as i64) < size {
                    let Value::Raw(chunk) = file.call("read", vec![Value::Int(bytes.len() as i64), Value::UInt(3)]).await? else { return Err("buffer expected".into()) };
                    bytes.extend(chunk);
                }
                Ok(Value::Bool(bytes == b"package bytes"))
            }),
    );
    let addr = node.listen("127.0.0.1:0").await.unwrap();
    let c = Client::connect(&addr.to_string()).await.unwrap();
    let file = choren_qi::object::file(b"package bytes".to_vec());
    assert_eq!(c.call("PackageManager", "install", vec![Value::Object(file)]).await.unwrap(), Value::Bool(true));
    assert!(c.call("PackageManager", "install", vec!["/tmp/x.pkg".into()]).await.unwrap_err().contains("path overload"));
}
