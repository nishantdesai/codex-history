check:
	cargo fmt --all -- --check
	cargo clippy --all-targets --all-features -- -D warnings
	cargo test --all-targets --all-features

fmt:
	cargo fmt --all

clippy:
	cargo clippy --all-targets --all-features -- -D warnings

test:
	cargo test --all-targets --all-features

package-release:
	./scripts/package-release.sh $$(rustc -vV | sed -n 's/^host: //p')

release-checksums:
	./scripts/generate-checksums.sh
