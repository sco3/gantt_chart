use chart_data::ChartData;
/// Generate a Gantt chart
use chrono::{Datelike, Duration, NaiveDate, NaiveDateTime, Weekday};
use clap::Parser;
use core::fmt::Arguments;
use easy_error::{self, bail, ResultExt};
use rand::prelude::*;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    fs::File,
    io::{self, Read, Write},
    path::PathBuf,
};
use svg::{
    node::{element::path::Data, Node, *},
    Document,
};
mod chart_data;
mod item_data;
mod log_macros;

static GOLDEN_RATIO_CONJUGATE: f32 = 0.618033988749895;
static MONTH_NAMES: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

#[derive(Parser)]
#[clap(version, about, long_about = None)]
struct Cli {
    /// Specify the JSON data file
    #[arg(value_name = "INPUT_FILE")]
    input_file: Option<PathBuf>,

    /// The SVG output file
    #[arg(value_name = "OUTPUT_FILE")]
    output_file: Option<PathBuf>,

    /// The width of the item title column
    #[arg(value_name = "WIDTH", short, long, default_value_t = 210.0)]
    title_width: f32,

    /// The maximum width of each month
    #[arg(value_name = "WIDTH", short, long, default_value_t = 80.0)]
    max_month_width: f32,

    /// Add a resource table at the bottom of the graph
    #[arg(short, long, default_value_t = false)]
    add_resource_table: bool,
}

impl Cli {
    fn get_output(&self) -> Result<Box<dyn Write>, Box<dyn Error>> {
        match self.output_file {
            Some(ref path) => File::create(path)
                .context(format!(
                    "Unable to create file '{}'",
                    path.to_string_lossy()
                ))
                .map(|f| Box::new(f) as Box<dyn Write>)
                .map_err(|e| Box::new(e) as Box<dyn Error>),
            None => Ok(Box::new(io::stdout())),
        }
    }

    fn get_input(&self) -> Result<Box<dyn Read>, Box<dyn Error>> {
        match self.input_file {
            Some(ref path) => File::open(path)
                .context(format!("Unable to open file '{}'", path.to_string_lossy()))
                .map(|f| Box::new(f) as Box<dyn Read>)
                .map_err(|e| Box::new(e) as Box<dyn Error>),
            None => Ok(Box::new(io::stdin())),
        }
    }
}

pub trait GanttChartLog {
    fn output(self: &Self, args: Arguments);
    fn warning(self: &Self, args: Arguments);
    fn error(self: &Self, args: Arguments);
}

pub struct GanttChartTool<'a> {
    log: &'a dyn GanttChartLog,
}

#[derive(Debug)]
pub struct Gutter {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Gutter {
    pub fn height(&self) -> f32 {
        self.bottom + self.top
    }

    pub fn width(&self) -> f32 {
        self.right + self.left
    }
}

#[derive(Debug)]
struct RenderData {
    title: String,
    gutter: Gutter,
    row_gutter: Gutter,
    row_height: f32,
    resource_gutter: Gutter,
    resource_height: f32,
    marked_date_offset: Option<f32>,
    title_width: f32,
    max_month_width: f32,
    rect_corner_radius: f32,
    styles: Vec<String>,
    cols: Vec<ColumnRenderData>,
    rows: Vec<RowRenderData>,
    resources: Vec<String>,
}

#[derive(Debug)]
struct RowRenderData {
    title: String,
    resource_index: usize,
    offset: f32,
    // If length not present then this is a milestone
    length: Option<f32>,
    open: bool,
}

#[derive(Debug)]
struct ColumnRenderData {
    width: f32,
    month_name: String,
}

impl<'a> GanttChartTool<'a> {
    pub fn new(log: &'a dyn GanttChartLog) -> GanttChartTool {
        GanttChartTool { log }
    }

    pub fn run(
        self: &mut Self,
        args: impl IntoIterator<Item = std::ffi::OsString>,
    ) -> Result<(), Box<dyn Error>> {
        let cli = match Cli::try_parse_from(args) {
            Ok(cli) => cli,
            Err(err) => {
                output!(self.log, "{}", err.to_string());
                return Ok(());
            }
        };

        let chart_data = Self::read_chart_file(cli.get_input()?)?;
        let render_data =
            self.process_chart_data(cli.title_width, cli.max_month_width, &chart_data)?;
        let document = self.render_chart(cli.add_resource_table, &render_data)?;

        Self::write_svg_file(cli.get_output()?, &document)?;
        Ok(())
    }

    fn read_chart_file(mut reader: Box<dyn Read>) -> Result<ChartData, Box<dyn Error>> {
        let mut content = String::new();

        reader.read_to_string(&mut content)?;

        let chart_data: ChartData = json5::from_str(&content)?;

        Ok(chart_data)
    }

    fn write_svg_file(writer: Box<dyn Write>, document: &Document) -> Result<(), Box<dyn Error>> {
        svg::write(writer, document)?;

        Ok(())
    }

    fn hsv_to_rgb(h: f32, s: f32, v: f32) -> u32 {
        let h_i = (h * 6.0) as usize;
        let f = h * 6.0 - h_i as f32;
        let p = v * (1.0 - s);
        let q = v * (1.0 - f * s);
        let t = v * (1.0 - (1.0 - f) * s);

        fn rgb(r: f32, g: f32, b: f32) -> u32 {
            ((r * 256.0) as u32) << 16 | ((g * 256.0) as u32) << 8 | ((b * 256.0) as u32)
        }

        if h_i == 0 {
            rgb(v, t, p)
        } else if h_i == 1 {
            rgb(q, v, p)
        } else if h_i == 2 {
            rgb(p, v, t)
        } else if h_i == 3 {
            rgb(p, q, v)
        } else if h_i == 4 {
            rgb(t, p, v)
        } else {
            rgb(v, p, q)
        }
    }

    fn process_chart_data(
        self: &Self,
        title_width: f32,
        max_month_width: f32,
        chart_data: &ChartData,
    ) -> Result<RenderData, Box<dyn Error>> {
        fn num_days_in_month(year: i32, month: u32) -> u32 {
            // the first day of the next month...
            let (y, m) = if month == 12 {
                (year + 1, 1)
            } else {
                (year, month + 1)
            };
            let d = NaiveDate::from_ymd(y, m, 1);

            // ...is preceded by the last day of the original month
            d.pred().day()
        }

        // Fail if only one task
        if chart_data.items.len() < 2 {
            bail!("You must provide more than one task");
        }

        let mut start_date = NaiveDateTime::MAX;
        let mut end_date = NaiveDateTime::MIN;
        let mut date = NaiveDateTime::MIN;
        let mut shadow_durations: Vec<Option<i64>> = Vec::with_capacity(chart_data.items.len());

        // Determine the project start & end dates
        for (i, item) in chart_data.items.iter().enumerate() {
            if let Some(item_start_date) = item.start_date {
                date = item_start_date;

                if item_start_date < start_date {
                    // Move the start if it falls on a weekend
                    start_date = match date.weekday() {
                        Weekday::Sat => date + Duration::days(2),
                        Weekday::Sun => date + Duration::days(1),
                        _ => date,
                    };
                }
            } else if i == 0 {
                return Err(From::from(format!("First item must contain a start date")));
            }

            // Skip the weekends and update a shadow list of the _real_ durations
            if let Some(item_days) = item.duration {
                let duration = match (date + Duration::days(item_days)).weekday() {
                    Weekday::Sat => Duration::days(item_days + 2),
                    Weekday::Sun => Duration::days(item_days + 1),
                    _ => Duration::days(item_days),
                };

                date += duration;

                shadow_durations.push(Some(duration.num_days()));
            } else {
                shadow_durations.push(None);
            }

            if end_date < date {
                end_date = date;
            }

            if let Some(item_resource_index) = item.resource_index {
                if item_resource_index >= chart_data.resources.len() {
                    return Err(From::from(format!("Resource index is out of range")));
                }
            } else if i == 0 {
                return Err(From::from(format!(
                    "First item must contain a resource index"
                )));
            }
        }

        start_date = NaiveDate::from_ymd(start_date.year(), start_date.month(), 1);
        end_date = NaiveDate::from_ymd(
            end_date.year(),
            end_date.month(),
            num_days_in_month(end_date.year(), end_date.month()),
        );

        // Create all the column data
        let mut all_items_width: f32 = 0.0;
        let mut num_item_days: u32 = 0;
        let mut cols = vec![];

        date = start_date;

        while date <= end_date {
            let item_days = num_days_in_month(date.year(), date.month());
            let item_width = max_month_width * (item_days as f32) / 31.0;

            num_item_days += item_days;
            all_items_width += item_width;

            cols.push(ColumnRenderData {
                width: item_width,
                month_name: MONTH_NAMES[date.month() as usize - 1].to_string(),
            });

            date = NaiveDate::from_ymd(
                date.year() + (if date.month() == 12 { 1 } else { 0 }),
                date.month() % 12 + 1,
                1,
            );
        }

        date = start_date;

        let mut resource_index: usize = 0;
        let gutter = Gutter {
            left: 10.0,
            top: 80.0,
            right: 10.0,
            bottom: 10.0,
        };
        let row_gutter = Gutter {
            left: 5.0,
            top: 5.0,
            right: 5.0,
            bottom: 5.0,
        };
        // TODO(john): The 20.0 should be configurable, and for the resource table
        let row_height = row_gutter.height() + 20.0;
        let resource_gutter = Gutter {
            left: 10.0,
            top: 10.0,
            right: 10.0,
            bottom: 10.0,
        };
        let resource_height = resource_gutter.height() + 20.0;
        let mut rows = vec![];

        // Calculate the X offsets of all the bars and milestones
        for (i, item) in chart_data.items.iter().enumerate() {
            if let Some(item_start_date) = item.start_date {
                date = item_start_date;
            }

            let offset = title_width
                + gutter.left
                + ((date - start_date).num_days() as f32) / (num_item_days as f32)
                    * all_items_width;

            let mut length: Option<f32> = None;

            if let Some(item_days) = shadow_durations[i] {
                // Use the shadow duration instead of the actual duration as it accounts for weekends
                date += Duration::days(item_days);
                length = Some((item_days as f32) / (num_item_days as f32) * all_items_width);
            }

            if let Some(item_resource_index) = item.resource_index {
                resource_index = item_resource_index;
            }

            rows.push(RowRenderData {
                title: item.title.clone(),
                resource_index,
                offset,
                length,
                open: item.open.unwrap_or(false),
            });
        }

        let marked_date_offset = if let Some(date) = chart_data.marked_date {
            // TODO(john): Put this offset calculation in a function
            Some(
                title_width
                    + gutter.left
                    + ((date - start_date).num_days() as f32) / (num_item_days as f32)
                        * all_items_width,
            )
        } else {
            None
        };

        let mut styles = vec![
            ".outer-lines{stroke-width:3;stroke:#aaaaaa;}".to_owned(),
            ".inner-lines{stroke-width:2;stroke:#dddddd;}".to_owned(),
            ".item{font-family:Arial;font-size:12pt;dominant-baseline:middle;}".to_owned(),
            ".resource{font-family:Arial;font-size:12pt;text-anchor:end;dominant-baseline:middle;}".to_owned(),
            ".title{font-family:Arial;font-size:18pt;}".to_owned(),
            ".heading{font-family:Arial;font-size:16pt;dominant-baseline:middle;text-anchor:middle;}".to_owned(),
            ".task-heading{dominant-baseline:middle;text-anchor:start;}".to_owned(),
            ".milestone{fill:black;stroke-width:1;stroke:black;}".to_owned(),
            ".marker{stroke-width:2;stroke:#888888;stroke-dasharray:7;}".to_owned(),
        ];

        // Generate random resource colors based on https://martin.ankerl.com/2009/12/09/how-to-create-random-colors-programmatically/
        let mut rng = rand::thread_rng();
        let mut h: f32 = rng.gen();

        for i in 0..chart_data.resources.len() {
            let rgb = GanttChartTool::hsv_to_rgb(h, 0.5, 0.5);

            styles.push(format!(
                ".resource-{}-closed{{fill:#{1:06x};stroke-width:1;stroke:#{1:06x};}}",
                i, rgb,
            ));
            styles.push(format!(
                ".resource-{}-open{{fill:none;stroke-width:2;stroke:#{1:06x};}}",
                i, rgb,
            ));

            h = (h + GOLDEN_RATIO_CONJUGATE) % 1.0;
        }

        Ok(RenderData {
            title: chart_data.title.to_owned(),
            gutter,
            row_gutter,
            row_height,
            resource_gutter,
            resource_height,
            styles,
            title_width,
            max_month_width,
            marked_date_offset,
            rect_corner_radius: 3.0,
            cols,
            rows,
            resources: chart_data.resources.clone(),
        })
    }

    fn render_chart(
        &self,
        add_resource_table: bool,
        rd: &RenderData,
    ) -> Result<Document, Box<dyn Error>> {
        let width: f32 = rd.gutter.left
            + rd.title_width
            + rd.cols.iter().map(|col| col.width).sum::<f32>()
            + rd.gutter.right;
        let height = rd.gutter.top
            + (rd.rows.len() as f32 * rd.row_height)
            + (if add_resource_table {
                rd.resource_gutter.height() + rd.resource_height
            } else {
                0.0
            })
            + rd.gutter.bottom;

        let mut document = Document::new()
            .set("viewbox", (0, 0, width, height))
            .set("xmlns", "http://www.w3.org/2000/svg")
            .set("width", width)
            .set("height", height)
            .set("style", "background-color: white;");
        let style = element::Style::new(rd.styles.join("\n"));

        // Render all the chart rows
        let mut rows = element::Group::new();

        for i in 0..=rd.rows.len() {
            let y = rd.gutter.top + (i as f32 * rd.row_height);

            rows.append(if i == 0 || i == rd.rows.len() {
                element::Line::new()
                    .set("class", "outer-lines")
                    .set("x1", rd.gutter.left)
                    .set("y1", y)
                    .set("x2", width - rd.gutter.right)
                    .set("y2", y)
            } else {
                element::Line::new()
                    .set("class", "inner-lines")
                    .set("x1", rd.gutter.left)
                    .set("y1", y)
                    .set("x2", width - rd.gutter.right)
                    .set("y2", y)
            });

            // Are we on one of the task rows?
            if i < rd.rows.len() {
                let row: &RowRenderData = &rd.rows[i];

                rows.append(
                    element::Text::new(&row.title)
                        .set("class", "item")
                        .set("x", rd.gutter.left + rd.row_gutter.left)
                        .set("y", y + rd.row_gutter.top + rd.row_height / 2.0),
                );

                // Is this a task or a milestone?
                if let Some(length) = row.length {
                    rows.append(
                        element::Rectangle::new()
                            .set(
                                "class",
                                format!(
                                    "resource-{}{}",
                                    row.resource_index,
                                    if row.open { "-open" } else { "-closed" }
                                ),
                            )
                            .set("x", row.offset)
                            .set("y", y + rd.row_gutter.top)
                            .set("rx", rd.rect_corner_radius)
                            .set("ry", rd.rect_corner_radius)
                            .set("width", length)
                            .set("height", rd.row_height - rd.row_gutter.height()),
                    );
                } else {
                    let n = (rd.row_height - rd.row_gutter.height()) / 2.0;
                    rows.append(
                        element::Path::new().set("class", "milestone").set(
                            "d",
                            Data::new()
                                .move_to((row.offset - n, y + rd.row_gutter.top + n))
                                .line_by((n, -n))
                                .line_by((n, n))
                                .line_by((-n, n))
                                .line_by((-n, -n)),
                        ),
                    );
                }
            }
        }

        // Render all the charts columns
        let mut columns = element::Group::new();

        for i in 0..=rd.cols.len() {
            let x: f32 = rd.gutter.left
                + rd.title_width
                + rd.cols.iter().take(i).map(|col| col.width).sum::<f32>();
            columns.append(
                element::Line::new()
                    .set("class", "inner-lines")
                    .set("x1", x)
                    .set("y1", rd.gutter.top)
                    .set("x2", x)
                    .set(
                        "y2",
                        rd.gutter.top + ((rd.rows.len() as f32) * rd.row_height),
                    ),
            );

            if i < rd.cols.len() {
                columns.append(
                    element::Text::new(&rd.cols[i].month_name)
                        .set("class", "heading")
                        .set("x", x + rd.max_month_width / 2.0)
                        .set(
                            "y",
                            // TODO(john): Use a more appropriate row height value here?
                            rd.gutter.top - rd.row_gutter.bottom - rd.row_height / 2.0,
                        ),
                );
            }
        }

        let tasks = element::Text::new("Tasks")
            .set("class", "heading task-heading")
            .set("x", rd.gutter.left + rd.row_gutter.left)
            .set(
                "y",
                rd.gutter.top - rd.row_gutter.bottom - rd.row_height / 2.0,
            );

        let title = element::Text::new(&rd.title)
            .set("class", "title")
            .set("x", rd.gutter.left)
            // TODO(john): Use more appropriate row height value here?
            .set("y", 25.0);

        let marker: Box<dyn Node> = if let Some(offset) = rd.marked_date_offset {
            Box::new(
                element::Line::new()
                    .set("class", "marker")
                    .set("x1", offset)
                    .set("y1", rd.gutter.top - 5.0)
                    .set("x2", offset)
                    .set(
                        "y2",
                        rd.gutter.top + ((rd.rows.len() as f32) * rd.row_height) + 5.0,
                    ),
            )
        } else {
            Box::new(element::Group::new())
        };

        let mut resources = element::Group::new();

        for i in 0..rd.resources.len() {
            if add_resource_table {
                let y = rd.gutter.top + ((rd.rows.len() as f32) * rd.row_height);
                let block_width = rd.resource_height - rd.resource_gutter.height();

                resources.append(
                    element::Text::new(&rd.resources[i])
                        .set("class", "resource")
                        .set(
                            "x",
                            rd.resource_gutter.left + ((i + 1) as f32) * 100.0 - 5.0,
                        )
                        .set("y", y + rd.resource_height / 2.0),
                );
                resources.append(
                    element::Rectangle::new()
                        .set("class", format!("resource-{}-closed", i))
                        .set(
                            "x",
                            rd.resource_gutter.left + ((i + 1) as f32) * 100.0 + 5.0,
                        )
                        .set("y", y + rd.resource_gutter.top)
                        .set("rx", rd.rect_corner_radius)
                        .set("ry", rd.rect_corner_radius)
                        .set("width", block_width)
                        .set("height", block_width),
                );
            }
        }

        document.append(style);
        document.append(title);
        document.append(columns);
        document.append(tasks);
        document.append(rows);
        document.append(marker);
        document.append(resources);

        Ok(document)
    }
}
