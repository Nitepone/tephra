assert(type(config.location) == "string" and config.location ~= "", "location: a place name in quotes")
assert(config.units == "metric" or config.units == "imperial", "units: \"metric\" or \"imperial\"")
