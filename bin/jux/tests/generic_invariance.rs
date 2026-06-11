//! Test for **generic invariance** (gap N2, §T.4 / §6.9.6): a same-name generic
//! type's arguments are invariant, so `Box<Dog>` is NOT a `Box<Animal>` even
//! though `Dog extends Animal` — the covariant upcast would let a caller write a
//! non-`Dog` through the `Box<Animal>` view. juxc must reject it with its own
//! E0410 diagnostic, not leak a rustc E0308. The same-type assignment still
//! compiles and runs.

use std::path::PathBuf;
use std::process::Command;

fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root resolves from bin/jux")
        .to_path_buf()
}

const ANIMALS: &str = r#"package inv;

public class Animal { public String name; public Animal(String name) { this.name = name; } }
public class Dog extends Animal { public Dog(String name) { super(name); } }
public class Box<T> {
    private T value;
    public Box(T value) { this.value = value; }
    public T get() { return this.value; }
    public void set(T v) { this.value = v; }
}
"#;

fn build(name: &str, body: &str) -> (bool, String) {
    let jux = env!("CARGO_BIN_EXE_jux");
    let dir = workspace_root().join("target").join(format!("it-inv-{name}"));
    std::fs::create_dir_all(&dir).expect("create probe dir");
    let src = dir.join("probe.jux");
    std::fs::write(&src, format!("{ANIMALS}\n{body}")).expect("write probe");
    let output = Command::new(jux)
        .arg("run")
        .arg("--emit-dir")
        .arg(dir.join("emit"))
        .arg(&src)
        .output()
        .expect("spawn jux");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    (output.status.success(), format!("{stdout}\n{stderr}"))
}

#[test]
fn covariant_generic_upcast_is_rejected() {
    let (ok, all) = build(
        "violation",
        r#"public void main() {
    var dogs = new Box<Dog>(new Dog("Rex"));
    Box<Animal> animals = dogs;   // invariance violation
    print(animals.get().name);
}
"#,
    );
    assert!(!ok, "covariant upcast unexpectedly compiled:\n{all}");
    assert!(all.contains("[E0410]"), "expected E0410, got:\n{all}");
    // The juxc diagnostic must fire instead of leaking a rustc type error.
    assert!(!all.contains("E0308"), "rustc type error leaked:\n{all}");
}

#[test]
fn invariant_same_type_assignment_runs() {
    let (ok, all) = build(
        "same",
        r#"public void main() {
    var dogs = new Box<Dog>(new Dog("Rex"));
    Box<Dog> same = dogs;          // invariant, same type: fine
    print(same.get().name);        // Rex
}
"#,
    );
    assert!(ok, "same-type generic assignment failed:\n{all}");
    assert!(all.contains("Rex"), "unexpected output:\n{all}");
}
