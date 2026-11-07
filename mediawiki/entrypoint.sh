#!/bin/sh
set -e
cd /var/www/html

if [ ! -f /config/LocalSettings.php ]; then
  for i in $(seq 1 30); do
    php maintenance/run.php install --dbtype=mysql --dbserver=db --dbname=wiki --dbuser=wiki --dbpass=wiki \
      --server=http://localhost:8080 --scriptpath= --lang=en --pass=wiki-admin-local \
      --confpath=/config Wiktionary Admin && break
    sleep 3
  done
  cat >> /config/LocalSettings.php <<'EOF'
wfLoadExtension( 'Scribunto' );
wfLoadExtension( 'ParserFunctions' );
wfLoadExtension( 'TemplateStyles' );
wfLoadExtension( 'Flow' );
wfLoadExtension( 'Cite' );
wfLoadExtension( 'Poem' );
wfLoadExtension( 'LabeledSectionTransclusion' );
wfLoadExtension( 'CharInsert' );
wfLoadExtension( 'wikihiero' );
$wgScribuntoDefaultEngine = 'luastandalone';
$wgScribuntoEngineConf['luastandalone']['luaPath'] = '/usr/bin/lua5.1';
// Wiktionary's Appendix namespace (and its talk pair). MediaWiki does not define it by default, so
// without these lines "Appendix:Glossary" is treated as a main-namespace title and the import skips it.
$wgExtraNamespaces[100] = 'Appendix';
$wgExtraNamespaces[101] = 'Appendix_talk';
// Wiktionary treats the first letter of a title as case-sensitive (Template:c and Template:C are different pages, and
// Module:Transclude compares template names to lowercase strings); MediaWiki's default capitalizes it.
$wgCapitalLinks = false;
EOF
  php maintenance/run.php update --quick
fi

until php -r 'exit(@mysqli_connect("db", "wiki", "wiki", "wiki") ? 0 : 1);'; do sleep 2; done

# Namespaces imported from the dump:
#   0   (Main)         dictionary entries; Module:Transclude reads entries from here, e.g. {{tcl|en|@ellip of:Surigao City}}
#   8   MediaWiki      interface and system message pages, e.g. MediaWiki:Scribunto-common-error-category
#   10  Template       template pages, e.g. Template:label, which {{lb}} and {{lbl}} redirect to
#   828 Module         Lua modules that templates run through {{#invoke:...}} (Scribunto)
#   829 Module talk    discussion pages attached to the modules
#   100 Appendix       Wiktionary appendix pages; Module:Get IDs reads Appendix:Glossary while rendering labels
# Namespace 101 (Appendix talk) is defined above but not imported.
if [ ! -f /config/.imported ]; then
  ( rm -f /config/.import-failed
    if php maintenance/run.php importDump --no-updates --report=1000 --namespaces "0|8|10|828|829|100" \
        /dump/enwiktionary-2026-10-01-p6p12232071.xml > /config/import.log 2>&1; then
      touch /config/.imported
    else
      echo $? > /config/.import-failed
    fi ) &
fi

exec apache2-foreground
