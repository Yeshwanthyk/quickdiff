# Homebrew Automation Setup

## One-time setup:

1. Create a **Personal Access Token** in GitHub:
   - Go to https://github.com/settings/tokens/new
   - Name: `HOMEBREW_TOOLS_TOKEN`
   - Scopes: `repo` (full control of private repositories)
   - Copy the token

2. Add to quickdiff secrets:
   - Go to https://github.com/Yeshwanthyk/quickdiff/settings/secrets/actions
   - Click "New repository secret"
   - Name: `HOMEBREW_TOOLS_TOKEN`
   - Value: paste the token from step 1

3. Ensure homebrew-tools repo exists and is accessible

## How it works:

When you create a release (e.g., `git tag v0.8.0 && git push --tags`):

1. Release.yml builds binaries → creates GitHub Release
2. Update-homebrew.yml triggers automatically
3. Downloads both binaries from release
4. Calculates SHA256 for each
5. Updates Formula/quickdiff.rb with new version + hashes
6. Commits and pushes to homebrew-tools

Users can then install with: `brew tap Yeshwanthyk/tools && brew install quickdiff`

## Manual update (if needed):

```bash
VERSION=0.8.0
ARM64_SHA=$(curl -sL "https://github.com/Yeshwanthyk/quickdiff/releases/download/v${VERSION}/quickdiff-aarch64-apple-darwin.tar.gz" | shasum -a 256 | awk '{print $1}')
LINUX_SHA=$(curl -sL "https://github.com/Yeshwanthyk/quickdiff/releases/download/v${VERSION}/quickdiff-x86_64-unknown-linux-gnu.tar.gz" | shasum -a 256 | awk '{print $1}')

# Then update Formula/quickdiff.rb with these values
```
