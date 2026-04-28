use crate::model::{NewsItem, RssItem};
use regex::Regex;
use std::collections::HashMap;

pub struct KeywordMatcher {
    patterns: Vec<Regex>,
    exclude_patterns: Vec<Regex>,
    keywords_count: HashMap<String, usize>,
}

impl KeywordMatcher {
    pub fn new(keywords: &[String], exclude_keywords: &[String]) -> crate::error::Result<Self> {
        let patterns: Vec<Regex> = keywords.iter()
            .filter_map(|k| Regex::new(&regex::escape(k)).ok())
            .collect();
        let exclude_patterns: Vec<Regex> = exclude_keywords.iter()
            .filter_map(|k| Regex::new(&regex::escape(k)).ok())
            .collect();

        Ok(KeywordMatcher {
            patterns,
            exclude_patterns,
            keywords_count: HashMap::new(),
        })
    }

    pub fn match_news(&self, item: &NewsItem) -> bool {
        let text = &item.title;
        if self.exclude_patterns.iter().any(|p| p.is_match(text)) {
            return false;
        }
        self.patterns.iter().any(|p| p.is_match(text))
    }

    pub fn match_rss(&self, item: &RssItem) -> bool {
        let text = &item.title;
        if self.exclude_patterns.iter().any(|p| p.is_match(text)) {
            return false;
        }
        self.patterns.iter().any(|p| p.is_match(text))
    }

    pub fn filter_news(&self, items: &[NewsItem]) -> Vec<NewsItem> {
        items.iter()
            .filter(|item| self.match_news(item))
            .cloned()
            .collect()
    }

    pub fn filter_rss(&self, items: &[RssItem]) -> Vec<RssItem> {
        items.iter()
            .filter(|item| self.match_rss(item))
            .cloned()
            .collect()
    }

    pub fn get_frequency(&self) -> &HashMap<String, usize> {
        &self.keywords_count
    }

    pub fn compute_frequency(&mut self, items: &[NewsItem]) {
        self.keywords_count.clear();
        for item in items {
            for pattern in &self.patterns {
                if pattern.is_match(&item.title) {
                    *self.keywords_count.entry(pattern.as_str().to_string()).or_insert(0) += 1;
                }
            }
        }
    }
}
