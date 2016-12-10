mod construction {
    use test;

    use {BufWriter, BufReader};

    use std::io;

    #[bench]
    fn bufreader(b: &mut test::Bencher) {
        b.iter(|| {
            BufReader::new(io::empty())
        });
    }

    #[bench]
    fn std_bufreader(b: &mut test::Bencher) {
        b.iter(|| {
            io::BufReader::new(io::empty())
        });
    }

    #[bench]
    fn bufwriter(b: &mut test::Bencher) {
        b.iter(|| {
            BufWriter::new(io::sink())
        });
    }

    #[bench]
    fn std_bufwriter(b: &mut test::Bencher) {
        b.iter(|| {
            io::BufWriter::new(io::sink())
        });
    }
}



