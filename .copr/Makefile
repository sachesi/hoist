TOPDIR := $(HOME)/rpmbuild
SOURCES_DIR := $(TOPDIR)/SOURCES
SPEC := hoist.spec
NAME := hoist
VERSION := $(shell rpmspec -q --qf '%{VERSION}\n' --srpm $(SPEC) | head -n1)

srpm:
	mkdir -p "$(SOURCES_DIR)"
	rm -rf vendor .cargo
	cargo vendor vendor >/dev/null
	tar --zstd -cf "$(SOURCES_DIR)/$(NAME)-$(VERSION)-vendor.tar.zst" vendor
	rm -rf vendor .cargo
	spectool -g -R $(SPEC)
	rpmbuild -bs $(SPEC)

clean:
	rm -rf vendor .cargo
