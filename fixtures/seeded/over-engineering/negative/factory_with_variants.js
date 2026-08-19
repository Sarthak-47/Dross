// Correct: the dispatch has real alternatives to choose between.
function createExporter(format) {
  switch (format) {
    case "csv":
      return new CsvExporter();
    case "json":
      return new JsonExporter();
    case "parquet":
      return new ParquetExporter();
  }
}
