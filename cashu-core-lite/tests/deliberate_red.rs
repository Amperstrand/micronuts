// Deliberate red run — evidence for micronuts#64 that CI can fail.
// This branch is never merged; closed unmerged after the red run.
#[test]
fn deliberate_red_ci_must_bite() {
    assert_eq!(1, 2, "deliberate red: proving checks can fail (micronuts#64)");
}
