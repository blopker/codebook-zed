This is the Codebook Zed extension.
See main repo at: [https://github.com/blopker/codebook](https://github.com/blopker/codebook)

Please submit any issues in the main repo.

To update the Zed Extension:

1. Update the version in extension.toml
1. Commit changes and tag with version, push
1. Move to zed/extensions repo
1. Checkout main and sync with: `git fetch upstream && git pull upstream main`
1. Make a new branch with extension version number (`gfb codebook-0.x.x`)
1. Run `git submodule update --remote --merge extensions/codebook` in zed/extensions
1. Update `extensions.toml` in zed/extensions with new version number.
1. Commit (`gc "Codebook v0.2.4"`), push
1. Make a PR to zed/extensions with the updated submodule
