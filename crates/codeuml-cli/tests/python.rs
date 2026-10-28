use std::fs;
use std::path::PathBuf;
use std::process::{Command, Output};

struct TempProject(PathBuf);

impl TempProject {
    fn new(label: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "codeuml-cli-python-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|elapsed| elapsed.as_nanos())
                .unwrap_or_default()
        ));
        fs::create_dir_all(&path).expect("temp project");
        Self(path)
    }

    fn write(&self, relative: &str, contents: &str) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("parent")).expect("directories");
        fs::write(path, contents).expect("write");
    }

    fn run(&self, arguments: &[&str]) -> Output {
        Command::new(env!("CARGO_BIN_EXE_codeuml"))
            .arg(&self.0)
            .args(arguments)
            .output()
            .expect("run codeuml")
    }
}

impl Drop for TempProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn python_project(label: &str) -> TempProject {
    let project = TempProject::new(label);
    project.write("pyproject.toml", "[project]\nname = \"zoo\"\n");
    project.write("zoo/__init__.py", "");
    project.write(
        "zoo/animals.py",
        "class Animal:\n    name: str\n\nclass Dog(Animal):\n    def owner(self) -> 'Person': ...\n",
    );
    project.write(
        "zoo/people.py",
        "from .animals import Dog\n\nclass Person:\n    pet: Dog\n",
    );
    project
}

#[test]
fn a_python_project_is_analyzed_without_any_python_installed_tooling() {
    let project = python_project("summary");
    let output = project.run(&[]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Source files: 3"), "{stdout}");
    assert!(stdout.contains("Types: 3"), "{stdout}");
    assert!(
        stdout.contains("Analysis: syntax only (3 analyzed, 0 failed)"),
        "{stdout}"
    );
    // Dog extends Animal, Dog.owner returns Person, Person.pet is a Dog.
    assert!(stdout.contains("Relationships: 3"), "{stdout}");
}

#[test]
fn python_projects_export_svg_by_module_and_by_neighborhood() {
    let project = python_project("svg");
    let module = project.run(&[
        "--format",
        "svg",
        "--scope",
        "module",
        "--module",
        "zoo.animals",
    ]);
    assert!(module.status.success(), "{module:?}");
    let svg = String::from_utf8_lossy(&module.stdout);
    assert!(svg.contains("Animal") && svg.contains("Dog"), "{svg}");
    assert!(svg.contains("Scope: Module: zoo.animals"), "{svg}");

    let neighborhood = project.run(&["--format", "svg", "--type", "zoo.people.Person"]);
    assert!(neighborhood.status.success(), "{neighborhood:?}");
    let svg = String::from_utf8_lossy(&neighborhood.stdout);
    assert!(svg.contains("Person") && svg.contains("Dog"), "{svg}");
}

#[test]
fn a_repository_with_both_languages_is_analyzed_into_one_model() {
    let project = python_project("mixed");
    project.write(
        "Cargo.toml",
        "[package]\nname = \"native\"\nversion = \"0.1.0\"\nedition = \"2021\"\n",
    );
    project.write("src/lib.rs", "pub struct Engine;\n");
    let output = project.run(&[]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Types: 4"), "{stdout}");
    assert!(stdout.contains("Source files: 4"), "{stdout}");
}

#[test]
fn broken_python_files_produce_a_partial_result_not_a_failure() {
    let project = python_project("partial");
    project.write("zoo/legacy.py", "def broken(:\n");
    let output = project.run(&[]);

    assert!(output.status.success(), "{output:?}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("Types: 3"), "{stdout}");
    assert!(stdout.contains("1 failed, partial"), "{stdout}");
    assert!(stdout.contains("Diagnostics: 1"), "{stdout}");
}

#[test]
fn directories_with_no_supported_language_list_what_was_expected() {
    let project = TempProject::new("none");
    project.write("notes.txt", "nothing to analyze");
    let output = project.run(&[]);

    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("no supported project"), "{stderr}");
    assert!(stderr.contains("Cargo.toml"), "{stderr}");
    assert!(stderr.contains("pyproject.toml"), "{stderr}");
}
