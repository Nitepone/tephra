assert(type(config.location) == "string" and config.location:trim() ~= "", "location: a US place in quotes, like \"Portland, OR\"")
assert(config.units == "metric" or config.units == "imperial", "units: \"metric\" or \"imperial\"")
assert(config.radar == "local" or config.radar == "regional" or config.radar == "off", "radar: \"local\", \"regional\" or \"off\"")
if config.zone ~= "" then time.format(0, "%H", config.zone) end -- raises if the zone is unknown
