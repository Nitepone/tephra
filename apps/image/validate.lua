-- Runs after the instance's config and before the app: stop with a clear message if it's wrong.
assert(config.url == nil or type(config.url) == "string", "url: a web address in quotes")
assert(config.url == nil or config.url == "" or config.url:match("^https?://"), "url: must start with http:// or https://")
assert(config.fit == "contain" or config.fit == "cover", "fit: \"contain\" or \"cover\"")
assert(type(config.dither) == "boolean", "dither: true or false")
