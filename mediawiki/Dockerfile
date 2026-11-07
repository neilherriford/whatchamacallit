FROM mediawiki:1.43

# Scribunto's bundled Lua binaries are x86 only; use the distribution's Lua 5.1 (the only version Scribunto supports).
RUN apt-get update \
 && apt-get install -y --no-install-recommends lua5.1 unzip \
 && rm -rf /var/lib/apt/lists/*

# Provides the sanitized-css content model used by Wiktionary's template stylesheets.
RUN git clone --depth 1 --branch REL1_43 https://gerrit.wikimedia.org/r/mediawiki/extensions/TemplateStyles extensions/TemplateStyles \
 && rm -rf extensions/TemplateStyles/.git

# Provides the flow-board content model; the import fails on any page whose model is unregistered.
# Flow needs pimple/pimple, which core's vendor/ lacks, so composer is used for it only.
COPY --from=composer:2 /usr/bin/composer /usr/local/bin/composer
RUN git clone --depth 1 --branch REL1_43 https://gerrit.wikimedia.org/r/mediawiki/extensions/Flow extensions/Flow \
 && rm -rf extensions/Flow/.git \
 && composer install --no-dev --no-interaction --working-dir=extensions/Flow \
 && rm /usr/local/bin/composer

# Tags and parser functions used by Wiktionary's templates and modules: <section>/{{#lsth:}}, <charinsert>, <hiero>.
RUN for e in LabeledSectionTransclusion CharInsert wikihiero; do \
      git clone --depth 1 --branch REL1_43 https://gerrit.wikimedia.org/r/mediawiki/extensions/$e extensions/$e \
      && rm -rf extensions/$e/.git; \
    done

COPY entrypoint.sh /entrypoint.sh
ENTRYPOINT ["sh", "/entrypoint.sh"]
