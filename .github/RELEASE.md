# Release Process

This document outlines how to release new versions of Weavegraph.

## Automated Release Process

### Prerequisites

1. **Set up crates.io token**: Add your crates.io API token as a GitHub secret:
   - Go to [crates.io/me](https://crates.io/me) and generate an API token
   - Add it to GitHub repository secrets as `CARGO_REGISTRY_TOKEN`
   - Settings → Secrets and variables → Actions → New repository secret

2. **Ensure all changes are merged**: Make sure all intended changes are on the `main` branch

### Creating a Release

1. **Go to GitHub Actions**: Navigate to the "Actions" tab in your repository

2. **Run Release Workflow**: 
   - Click on "Release" workflow
   - Click "Run workflow"
   - Enter the version number (e.g., `0.1.0`, `0.2.0-beta.1`)
   - Click "Run workflow"

3. **Automatic Process**: The workflow will:
   - Validate the version format
   - Update `Cargo.toml` with the new version
   - Commit and push the version bump
   - Create and push a git tag
   - Create a GitHub release with auto-generated changelog
   - Trigger the CI/CD pipeline which will publish to crates.io

## Manual Release Process (if needed)

If you prefer to release manually:

```bash
# 1. Update version in Cargo.toml
vim Cargo.toml

# 2. Commit the version bump
git add Cargo.toml
git commit -m "chore: bump version to 0.1.0"
git push

# 3. Create and push tag
git tag v0.1.0
git push origin v0.1.0

# 4. Create GitHub release (triggers auto-publish)
# Use GitHub web interface or gh CLI:
gh release create v0.1.0 --title "v0.1.0" --generate-notes

# 5. Publish to crates.io (if auto-publish fails)
cargo publish
```

## Version Numbering

We follow [Semantic Versioning](https://semver.org/):

- **PATCH** (0.1.1): Bug fixes, documentation updates
- **MINOR** (0.2.0): New features, backward-compatible changes  
- **MAJOR** (1.0.0): Breaking changes, API changes

### Pre-release versions:
- **Alpha**: `0.2.0-alpha.1` - Early development, unstable
- **Beta**: `0.2.0-beta.1` - Feature complete, testing phase
- **RC**: `0.2.0-rc.1` - Release candidate, final testing

## Pre-Release Checklist

Before creating a release:

- [ ] All tests pass locally: `cargo test --all`
- [ ] Code is properly formatted: `cargo fmt --check`
- [ ] No clippy warnings: `cargo clippy --all-targets --all-features`
- [ ] Examples compile: `cargo check --examples`
- [ ] Documentation is up to date
- [ ] CHANGELOG.md is updated (if maintained manually)
- [ ] Version number follows semantic versioning
- [ ] No known security vulnerabilities: `cargo audit`

## Post-Release

After a successful release:

1. **Verify publication**: Check that the new version appears on [crates.io/crates/weavegraph](https://crates.io/crates/weavegraph)

2. **Test installation**: Verify users can install the new version:
   ```bash
   cargo install weavegraph --version 0.1.0
   ```

3. **Update documentation**: Ensure docs.rs builds successfully

4. **Announce**: Consider announcing on relevant platforms (Reddit, Discord, etc.)

## Troubleshooting

### Common Issues

**Version mismatch error**: Ensure the version in `Cargo.toml` matches the git tag exactly.

**Publish fails**: Check that:
- The crates.io token is valid and has publish permissions
- All dependencies are available on crates.io
- The package name isn't already taken

**CI fails**: Check the GitHub Actions logs for specific error messages.

### Emergency Fixes

If you need to yank a version:
```bash
cargo yank --version 0.1.0
# To undo:
cargo yank --version 0.1.0 --undo
```

## GitHub Secrets Required

- `CARGO_REGISTRY_TOKEN`: Your crates.io API token for publishing

The `GITHUB_TOKEN` is automatically provided by GitHub Actions.