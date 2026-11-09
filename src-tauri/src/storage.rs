use std::collections::BTreeMap;

pub type SiteStorage = BTreeMap<String, String>;

pub const MAX_ITEM_BYTES: usize = 32 * 1024;
pub const MAX_TOTAL_BYTES: usize = 512 * 1024;

const ORIGIN_SLOT: &str = "__CHATLY_ORIGIN__";
const MAX_SLOT: &str = "__CHATLY_MAX__";
const ITEMS_SLOT: &str = "__CHATLY_ITEMS__";

const READ: &str = "(function(){try{if(location.origin!==__CHATLY_ORIGIN__)return '';var o={};\
for(var i=0;i<localStorage.length;i++){var k=localStorage.key(i);if(k===null)continue;\
var v=localStorage.getItem(k);if(v!==null&&k.length+v.length<=__CHATLY_MAX__)o[k]=v}\
return ''+JSON.stringify(o)}catch(e){return ''}})()";

const RESTORE: &str = "(function(){try{if(location.origin!==__CHATLY_ORIGIN__)return;\
if(sessionStorage.getItem('chatly-restored'))return;sessionStorage.setItem('chatly-restored','1');\
if(localStorage.length)return;var d=JSON.parse(__CHATLY_ITEMS__);\
Object.keys(d).forEach(function(k){localStorage.setItem(k,d[k])})}catch(e){}})();";

fn js_string(s: &str) -> String {
    serde_json::to_string(s).expect("a string serializes")
}

pub fn read_script(origin: &str) -> String {
    READ.replace(ORIGIN_SLOT, &js_string(origin))
        .replace(MAX_SLOT, &MAX_ITEM_BYTES.to_string())
}

pub fn parse(result: &str) -> Option<SiteStorage> {
    let json: String = serde_json::from_str(result).ok()?;
    let items: SiteStorage = serde_json::from_str(&json).ok()?;
    Some(capped(items))
}

fn capped(items: SiteStorage) -> SiteStorage {
    let mut total = 0;
    items
        .into_iter()
        .filter(|(k, v)| {
            let size = k.len() + v.len();
            if size > MAX_ITEM_BYTES || total + size > MAX_TOTAL_BYTES {
                return false;
            }
            total += size;
            true
        })
        .collect()
}

pub fn restore_script(origin: &str, items: &SiteStorage) -> String {
    let json = serde_json::to_string(items).expect("a map of strings serializes");
    RESTORE
        .replace(ORIGIN_SLOT, &js_string(origin))
        .replace(ITEMS_SLOT, &js_string(&json))
}

#[cfg(test)]
mod tests {
    use super::*;

    const ORIGIN: &str = "https://web.telegram.org";

    fn items(pairs: &[(&str, &str)]) -> SiteStorage {
        pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    fn as_eval_result(map: &SiteStorage) -> String {
        js_string(&serde_json::to_string(map).unwrap())
    }

    #[test]
    fn parses_what_the_read_script_returns() {
        let map = items(&[("dc", "2"), ("dc2_auth_key", "\"abcdef\""), ("user_auth", "{\"dcID\":2}")]);
        assert_eq!(parse(&as_eval_result(&map)), Some(map));
        assert_eq!(parse(&js_string("{}")), Some(SiteStorage::new()));
    }

    #[test]
    fn unusable_results_keep_the_old_storage() {
        assert_eq!(parse(""), None, "null or a thrown error");
        assert_eq!(parse("\"\""), None, "wrong origin");
        assert_eq!(parse("{\"dc\":\"2\"}"), None, "an object instead of a string");
        assert_eq!(parse(&js_string("[1,2]")), None);
        assert_eq!(parse(&js_string("{\"dc\":2}")), None, "values must be strings");
        assert_eq!(parse("42"), None);
    }

    #[test]
    fn oversized_items_are_dropped() {
        let big = "x".repeat(MAX_ITEM_BYTES);
        let map = items(&[("big", &big), ("dc", "2")]);
        assert_eq!(parse(&as_eval_result(&map)), Some(items(&[("dc", "2")])));
    }

    #[test]
    fn total_is_capped_in_key_order() {
        let chunk = "y".repeat(MAX_ITEM_BYTES - 8);
        let map: SiteStorage = (0..40).map(|i| (format!("k{i:02}"), chunk.clone())).collect();
        let kept = parse(&as_eval_result(&map)).unwrap();
        let total: usize = kept.iter().map(|(k, v)| k.len() + v.len()).sum();
        assert!(total <= MAX_TOTAL_BYTES);
        assert_eq!(kept.len(), MAX_TOTAL_BYTES / MAX_ITEM_BYTES);
        assert!(kept.contains_key("k00") && !kept.contains_key("k39"));
    }

    #[test]
    fn scripts_check_the_origin() {
        let read = read_script(ORIGIN);
        assert!(read.contains("location.origin!==\"https://web.telegram.org\""));
        assert!(read.contains(&format!("<={MAX_ITEM_BYTES}")));
        assert!(read.contains("return ''+JSON.stringify(o)"));
        let restore = restore_script(ORIGIN, &items(&[("dc", "2")]));
        assert!(restore.contains("location.origin!==\"https://web.telegram.org\""));
        for script in [&read, &restore] {
            assert!(!script.contains("__CHATLY_"), "placeholder left in {script}");
        }
    }

    #[test]
    fn restore_script_carries_values_as_one_string_literal() {
        let tricky = items(&[("a\"b", "</script><script>alert(1)</script>"), ("c\\d", "line\nbreak\u{2028}")]);
        let script = restore_script(ORIGIN, &tricky);
        let start = script.find("JSON.parse(").unwrap() + "JSON.parse(".len();
        let end = script.find(");Object.keys").unwrap();
        let literal: String = serde_json::from_str(&script[start..end]).unwrap();
        let back: SiteStorage = serde_json::from_str(&literal).unwrap();
        assert_eq!(back, tricky);
        assert!(script.contains("sessionStorage.getItem('chatly-restored')"));
        assert!(script.contains("if(localStorage.length)return"));
    }
}
