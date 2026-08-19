// Defect: factory scaffolding that dispatches to exactly one variant, so the
// dispatch never actually chooses anything.
function createExporter(format) {
  switch (format) {
    case "csv":
      return new CsvExporter();
  }
}
