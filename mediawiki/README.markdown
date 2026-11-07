# Overview
This container is for rendering templates

## Building and Running
### Prerequisites
Download an XML dump file, and place it in the `dictionary/` directory

### Building

Use the associated `makefile`

```bash
make build
make run
```

This starts an import from the dump.  Check the import status with `make status`.


## Rendering templates
```bash
template='{{lb|en|intransitive}}'
page_title='Nonsense'

curl -s http://localhost:8080/api.php \
  --data-urlencode 'action=expandtemplates' \
  --data-urlencode "title=$page_title" \
  --data-urlencode "text=$template" \
  --data-urlencode 'prop=wikitext' \
  --data-urlencode 'format=json'
```
