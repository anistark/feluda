#' Analyze Sample Dataset
#'
#' This function demonstrates basic data manipulation and visualization
#' using popular R packages (dplyr, ggplot2, tidyr, readr).
#'
#' @param data A data frame to analyze
#' @return A ggplot2 visualization object
#' @export
#'
#' @examples
#' analyze_sample_data(mtcars)
analyze_sample_data <- function(data) {
  library(dplyr)
  library(ggplot2)
  library(tidyr)
  library(readr)

  # Data manipulation with dplyr
  summary_data <- data %>%
    group_by_at(1) %>%
    summarise(across(where(is.numeric), mean, na.rm = TRUE)) %>%
    pivot_longer(cols = -1, names_to = "metric", values_to = "value")

  # Visualization with ggplot2
  plot <- ggplot(summary_data, aes(x = metric, y = value)) +
    geom_col(fill = "steelblue") +
    theme_minimal() +
    labs(
      title = "Data Summary",
      x = "Metric",
      y = "Average Value"
    ) +
    theme(axis.text.x = element_text(angle = 45, hjust = 1))

  return(plot)
}

#' Process CSV File
#'
#' Load and process a CSV file using readr
#'
#' @param file_path Path to CSV file
#' @return Processed data frame
#' @export
process_csv <- function(file_path) {
  data <- read_csv(file_path, show_col_types = FALSE)

  # Basic data cleaning
  data_clean <- data %>%
    drop_na()

  return(data_clean)
}
