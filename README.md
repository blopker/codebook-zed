This is the Codebook Zed extension.
See main repo at: [https://github.com/blopker/codebook](https://github.com/blopker/codebook)

Please submit any issues in the main repo.

## Release process

First, update the change log, commit and push. Don't update version in extension.toml.

Run the release helper to push a new version of the extension and update the
`zed-extensions` repository:

```sh
bun scripts/release.ts 0.2.4
```

What the script does:

1. Updates `extension.toml`, commits every change in this repo with the message `Codebook v0.2.4`, tags the commit, and pushes both the commit and the tag.
2. Switches to `../zed-extensions`, checks out `main`, fetches/pulls `upstream main`, and creates a branch named `codebook-0.2.4`.
3. Runs `git submodule update --remote --merge extensions/codebook`, bumps the version entry in `extensions.toml`, commits, and pushes the branch.
4. Prints a reminder to open a PR in `zed-extensions` for the newly pushed branch.

Requirements:

- Bun installed locally.
- The `../zed-extensions` repo exists and has an `upstream` remote configured.
- Both repositories are free of uncommitted changes (other than what you plan to include in the release in this repo).
