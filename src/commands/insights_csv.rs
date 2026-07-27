#[derive(Debug, Clone)]
pub(crate) struct CsvMetricRow {
    section: String,
    rank: String,
    metric: String,
    value: String,
    process: String,
    command_family: String,
    command: String,
    day: String,
    invocation_id: String,
    source: String,
    exit_code: String,
    argv_json: String,
    command_shape: String,
    command_shape_hash: String,
}

impl CsvMetricRow {
    pub(crate) fn new(
        section: impl Into<String>,
        metric: impl Into<String>,
        value: impl ToString,
    ) -> Self {
        Self {
            section: section.into(),
            rank: String::new(),
            metric: metric.into(),
            value: value.to_string(),
            process: String::new(),
            command_family: String::new(),
            command: String::new(),
            day: String::new(),
            invocation_id: String::new(),
            source: String::new(),
            exit_code: String::new(),
            argv_json: String::new(),
            command_shape: String::new(),
            command_shape_hash: String::new(),
        }
    }

    pub(crate) fn rank(mut self, rank: usize) -> Self {
        self.rank = rank.to_string();
        self
    }

    pub(crate) fn section(mut self, section: impl Into<String>) -> Self {
        self.section = section.into();
        self
    }

    pub(crate) fn metric(mut self, metric: impl Into<String>, value: impl ToString) -> Self {
        self.metric = metric.into();
        self.value = value.to_string();
        self
    }

    pub(crate) fn command(mut self, command: &str) -> Self {
        self.command = command.to_string();
        self
    }

    pub(crate) fn command_family(mut self, command_family: &str) -> Self {
        self.command_family = command_family.to_string();
        self
    }

    pub(crate) fn process(mut self, process: &str) -> Self {
        self.process = process.to_string();
        self
    }

    pub(crate) fn command_root(mut self, command_root: &str) -> Self {
        self.process = command_root.to_string();
        self
    }

    pub(crate) fn day(mut self, day: &str) -> Self {
        self.day = day.to_string();
        self
    }

    pub(crate) fn invocation_id(mut self, invocation_id: u64) -> Self {
        self.invocation_id = invocation_id.to_string();
        self
    }

    pub(crate) fn source(mut self, source: &str) -> Self {
        self.source = source.to_string();
        self
    }

    pub(crate) fn exit_code(mut self, exit_code: i32) -> Self {
        self.exit_code = exit_code.to_string();
        self
    }

    pub(crate) fn argv_json(mut self, argv_json: &str) -> Self {
        self.argv_json = argv_json.to_string();
        self
    }

    pub(crate) fn command_shape(mut self, command_shape: &str) -> Self {
        self.command_shape = command_shape.to_string();
        self
    }

    pub(crate) fn command_shape_hash(mut self, command_shape_hash: &str) -> Self {
        self.command_shape_hash = command_shape_hash.to_string();
        self
    }
}

pub(crate) fn push_metric_row(output: &mut String, row: CsvMetricRow) {
    let fields = [
        row.section,
        row.rank,
        row.metric,
        row.value,
        row.process,
        row.command_family,
        row.command,
        row.day,
        row.invocation_id,
        row.source,
        row.exit_code,
        row.argv_json,
        row.command_shape,
        row.command_shape_hash,
    ];
    for (index, field) in fields.iter().enumerate() {
        if index > 0 {
            output.push(',');
        }
        output.push_str(&csv_cell(field));
    }
    output.push('\n');
}

fn csv_cell(field: &str) -> String {
    if field.contains([',', '"', '\n', '\r']) {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}
