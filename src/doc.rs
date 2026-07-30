use std::collections::HashMap;

use crate::dxf::entities::{BlockRecord, RawEntity, read_blocks, read_section};
use crate::dxf::lexer::lex;
use crate::dxf::tables::{
    HeaderVars, LayerRecord, LtypeRecord, read_header, read_layers, read_ltypes,
};

#[derive(Debug)]
pub struct Document {
    pub header: HeaderVars,
    pub layers: HashMap<String, LayerRecord>,
    pub ltypes: HashMap<String, LtypeRecord>,
    pub blocks: HashMap<String, BlockRecord>,
    /// Model-space and paper-space entities from the ENTITIES section only.
    /// Block bodies live in `blocks` and are reached through INSERT.
    pub entities: Vec<RawEntity>,
}

impl Document {
    pub fn parse(bytes: &[u8]) -> Result<Document, String> {
        let pairs = lex(bytes)?;
        let header = read_header(&pairs);
        let cp = header.codepage;
        Ok(Document {
            layers: read_layers(&pairs, cp),
            ltypes: read_ltypes(&pairs, cp),
            blocks: read_blocks(&pairs, cp),
            entities: read_section(&pairs, "ENTITIES"),
            header,
        })
    }

    pub fn layer(&self, name: &str) -> Option<&LayerRecord> {
        self.layers.get(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::encoding::Codepage;

    const SRC: &[u8] = b"  0\nSECTION\n  2\nHEADER\n  9\n$DWGCODEPAGE\n  3\nANSI_936\n  0\nENDSEC\n  0\nSECTION\n  2\nTABLES\n  0\nTABLE\n  2\nLAYER\n  0\nLAYER\n  2\nWALL\n 62\n4\n370\n35\n  6\nCONTINUOUS\n  0\nENDTAB\n  0\nENDSEC\n  0\nSECTION\n  2\nBLOCKS\n  0\nBLOCK\n  2\nFRAME\n  0\nLINE\n  8\nWALL\n 10\n0.0\n 20\n0.0\n 11\n10.0\n 21\n0.0\n  0\nENDBLK\n  0\nENDSEC\n  0\nSECTION\n  2\nENTITIES\n  0\nLINE\n  8\nWALL\n 10\n1.0\n 20\n2.0\n 11\n3.0\n 21\n4.0\n370\n15\n  0\nINSERT\n  8\n0\n  2\nFRAME\n 10\n100.0\n 20\n200.0\n  0\nENDSEC\n  0\nEOF\n";

    #[test]
    fn parses_a_document_end_to_end() {
        let d = Document::parse(SRC).expect("parse should succeed");
        assert_eq!(d.header.codepage, Codepage::Gbk);
        assert_eq!(d.layers["WALL"].lineweight, 35);
        assert_eq!(d.blocks["FRAME"].entities.len(), 1);
        assert_eq!(d.entities.len(), 2);
    }

    #[test]
    fn entity_accessors_read_group_codes() {
        let d = Document::parse(SRC).unwrap();
        let line = &d.entities[0];
        assert_eq!(line.kind, "LINE");
        assert_eq!(line.layer(d.header.codepage), "WALL");
        assert_eq!(line.f64(10, 0.0), 1.0);
        assert_eq!(line.f64(21, 0.0), 4.0);
        assert_eq!(line.int(370, -1), 15);
        assert_eq!(line.int(62, 256), 256, "absent codes return the default");
    }

    #[test]
    fn insert_carries_its_block_name() {
        let d = Document::parse(SRC).unwrap();
        let insert = &d.entities[1];
        assert_eq!(insert.kind, "INSERT");
        assert_eq!(insert.text(2, d.header.codepage).as_deref(), Some("FRAME"));
    }

    #[test]
    fn block_contents_are_not_leaked_into_root_entities() {
        // The BLOCKS section also contains a LINE. It must not appear in
        // `entities`, or every block body would be drawn twice: once at the
        // origin and once through its INSERT.
        let d = Document::parse(SRC).unwrap();
        assert_eq!(d.entities.iter().filter(|e| e.kind == "LINE").count(), 1);
    }
}
