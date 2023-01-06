# Git Workflow
* Commit style summary ([source](https://cbea.ms/git-commit/)):
  * Separate subject from body with a blank line.
  * Limit the subject line to 50 characters.
  * Capitalize the subject line.
  * Do not end the subject line with a period.
  * Use the imperative mood in the subject line.
  * Wrap the body at 72 characters.
  * Use the body to explain what and why vs. how.
* Commits should be [atomic](https://www.freshconsulting.com/insights/blog/atomic-commits/):
  * An atomic commit always leaves the code in a compilable state.
  * Make commits often and small.
* Branches: 
  * Namn: Issue ID + issue name in `kebab-case-like-this`, e.g. `18-system-scheduling` (*18* is the issue ID from GitHub, *System Scheduling* is the name of the issue).
  * Every task is worked on in a separate branch and is merged back into master through a pull request (PR) that is reviewed.
* All features, bugs and milestones are stored on GitHub within the Project, and is linked to the code and all PRs.
* The `master` branch is protected and can only be modified through reviewed PRs. 
