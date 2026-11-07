.PHONY: build run stop clean status reimport

build:
	docker compose build

run:
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
	elif [ -f config/.import-failed ]; then \
	  echo "import FAILED (exit code $$(cat config/.import-failed)); last lines of config/import.log:"; tail -n 20 config/import.log; \
	else \
	  docker compose exec -T mediawiki sh -c 'p=$$(pgrep -f "[i]mportDump" | head -1); [ -n "$$p" ] || { echo "import not running"; exit 0; }; for f in /proc/$$p/fd/*; do case "$$(readlink $$f)" in *.xml) pos=$$(awk "/^pos:/{print \$$2}" /proc/$$p/fdinfo/$$(basename $$f)); size=$$(stat -L -c %s $$f); echo "import $$((pos * 100 / size))% ($$pos of $$size bytes read)";; esac; done'; \
	fi
