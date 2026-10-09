mod support;

use support::{run_in, stderr, TempProject, VALID_STELLAR_VALUE_BASE64};

const OFFLINE_CONFIG: &str =
    "version = 1\nprotocol = 28\n\n[tests]\nxdr = true\nrpc = false\nsoroban = false\n";

fn xdr_fixture(id: &str, extra: &str) -> String {
    format!(
        "id = \"{id}\"\nprotocol = 28\nsurface = \"xdr\"\ncategory = \"test\"\ndescription = \"test\"\ntype = \"StellarValue\"\nkind = \"decode-success\"\nvalue_base64 = \"{VALID_STELLAR_VALUE_BASE64}\"\n{extra}"
    )
}

#[test]
fn a_payload_reference_that_escapes_the_fixture_directory_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-escape");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("secret.bin", "outside the fixtures directory");
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "input_file = \"../secret.bin\"\n"),
    );

    for command in ["check", "inspect", "fixtures"] {
        let output = run_in(&dir.path, &[command]);
        assert_eq!(output.status.code(), Some(4), "{command}");
        let text = stderr(&output);
        assert!(text.contains("unsafe input_file"), "{command}: {text}");
        assert!(text.contains(".."), "{command}: {text}");
    }
}

#[test]
fn an_absolute_payload_reference_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-absolute");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write(
        "fixtures/p28-xdr-1.toml",
        &xdr_fixture("p28-xdr-1", "expected_file = \"/etc/hostname\"\n"),
    );

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("absolute"));
}

#[cfg(unix)]
#[test]
fn a_symlink_inside_the_fixture_directory_is_an_invalid_fixture() {
    let dir = TempProject::new("fixture-safety-symlink");
    dir.write(".stellar-canary.toml", OFFLINE_CONFIG);
    dir.write("elsewhere/p28-xdr-1.toml", &xdr_fixture("p28-xdr-1", ""));
    std::fs::create_dir_all(dir.path.join("fixtures")).unwrap();
    std::os::unix::fs::symlink(dir.path.join("elsewhere"), dir.path.join("fixtures/linked"))
        .unwrap();

    let output = run_in(&dir.path, &["check"]);
    assert_eq!(output.status.code(), Some(4));
    assert!(stderr(&output).contains("symbolic link"));
}
