.PHONY: build plan run stop clean status reimport

build:
	docker compose build

plan:
	@mkdir -p config
	@if [ -f config/.imported ]; then echo "import already complete"; else cargo run --release --quiet --manifest-path ../Cargo.toml -- split-dump --ranges-only --out config; fi

run: plan
	docker compose up -d

stop:
	docker compose down

clean:
	docker compose down -v
	rm -rf config

reimport:
	$(MAKE) clean
	$(MAKE) build
	$(MAKE) run

status:
	@if [ -f config/.imported ]; then echo "import complete"; \
	elif [ -f config/.import-failed ]; then echo "import FAILED:"; cat config/.import-failed; \
	elif [ -f config/ranges.tsv ]; then \
	  total=$$(( $$(wc -l < config/ranges.tsv) - 1 )); \
	  done=$$(ls config/import-logs/*.done 2> /dev/null | wc -l | tr -d ' '); \
	  echo "import $$done of $$total parts done"; \
	else echo "import not started (run: make run)"; fi
