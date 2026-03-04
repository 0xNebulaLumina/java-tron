Find the first undone item in yolo/input.md, and its line number, let's say it's at line 23 (1-based) and it's `TRANSFER_CONTRACT`,
then checkout the current branch to a new branch `Theseus_compare_review_again_<LINE_NUM>_<CONTRACT_NAME>` (`Theseus_compare_review_again_23_TRANSFER_CONTRACT>` in this example).

Then:

I have a design plan in planning/review_again/<CONTRACT_NAME>.planning.md and planning/review_again/<CONTRACT_NAME>.todo.md, which you will implement accordingly.

Some rules:
* Always strive for full parity.
* If any fix, feature, or test (except end-to-end tests) in planning/review_again/<CONTRACT_NAME>.todo.md is not completed, add them proactively so I don't have to ask you.
* Remember to test the new code (useful test commands: `./scripts/ci/run_fixture_conformance.sh --rust-only`, `cd rust-backend; cargo test --workspace`).
* Update the to-do list/checklist progress in planning/review_again/<CONTRACT_NAME>.todo.md as you complete each step.
* Double-check for any items marked as not done in planning/review_again/<CONTRACT_NAME>.todo.md that have actually been completed, and vice versa.

Your code will be reviewed by GPT, so make sure to implement it carefully.
