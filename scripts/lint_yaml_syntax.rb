#!/usr/bin/env ruby

require "find"
require "yaml"

ROOT = Dir.pwd
SKIP_DIRS = [".git", "target"].freeze
FILES = []

Find.find(ROOT) do |path|
  if File.directory?(path)
    name = File.basename(path)
    if SKIP_DIRS.include?(name)
      Find.prune
    else
      next
    end
  end

  next unless path.end_with?(".yml", ".yaml")

  relative = path.sub("#{ROOT}/", "")
  FILES << relative
end

errors = 0

FILES.sort.each do |relative|
  begin
    YAML.safe_load(File.read(relative), aliases: true)
  rescue StandardError => err
    errors += 1
    warn "YAML syntax error in #{relative}: #{err.message}"
  end
end

if errors.positive?
  warn "YAML lint fallback failed with #{errors} file(s) containing syntax errors."
  exit 1
end

puts "YAML lint fallback passed (syntax validation only)."
