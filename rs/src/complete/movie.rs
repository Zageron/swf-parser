use crate::complete::parse_tag;
use crate::streaming::movie::parse_swf_signature;
use nom::IResult as NomResult;
use swf_types as ast;

/// Represents the possible parse errors when parsing an SWF file.
///
/// Fatal errors can only occur at the beginning of the parsing. Once the header
/// is parsed, the tags are always parsed successfully. Invalid tags produce
/// `Raw` tags but don't prevent the parser from completing: the parser is
/// resilient to invalid (or unknown) tags.
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum SwfParseError {
  /// Indicates an invalid SWF signature.
  ///
  /// The SWF signature corresponds to the first 8 bytes of the movie.
  /// This error occurs either if there is not enough data to even parse
  /// the signature or if the compression method is invalid.
  InvalidSignature,

  /// Indicates a failure to decompress the payload.
  ///
  /// The payload represents all the data following the SWF signature.
  /// If the SWF file uses a compressed payload (`Deflate` or `Lzma`), this
  /// error is emitted when the decompression fails for any reason.
  InvalidPayload,

  /// Indicates an invalid movie header.
  ///
  /// The movie header corresponds to the first few bytes of the payload.
  /// This error occurs if there is not enough data to parse the header.
  InvalidHeader,
}

/// Parses a completely loaded SWF file.
///
/// See [[SwfParseError]] for details on the possible errors.
///
/// This function never panics.
pub fn parse_swf(input: &[u8]) -> Result<ast::Movie, SwfParseError> {
  let (input, signature) = match parse_swf_signature(input) {
    Ok(ok) => ok,
    Err(_) => return Err(SwfParseError::InvalidSignature),
  };

  let mut movie_buffer: Vec<u8>;

  let movie_bytes: &[u8] = match signature.compression_method {
    ast::CompressionMethod::None => input,
    ast::CompressionMethod::Deflate => {
      movie_buffer = match inflate::inflate_bytes_zlib(input) {
        Ok(uncompressed) => uncompressed,
        Err(_) => return Err(SwfParseError::InvalidPayload),
      };
      &movie_buffer
    }
    ast::CompressionMethod::Lzma => {
      let mut payload_reader = std::io::BufReader::new(input);
      movie_buffer = Vec::new();
      match lzma_rs::lzma_decompress(&mut payload_reader, &mut movie_buffer) {
        Ok(_) => (),
        Err(_) => return Err(SwfParseError::InvalidPayload),
      }
      &movie_buffer
    }
  };

  let (_, movie) = match parse_movie(movie_bytes, signature.swf_version) {
    Ok(ok) => ok,
    Err(_) => return Err(SwfParseError::InvalidHeader),
  };

  Ok(movie)
}

/// Parses a completely loaded movie.
///
/// The movie is the uncompressed payload of the SWF.
fn parse_movie(input: &[u8], swf_version: u8) -> NomResult<&[u8], ast::Movie> {
  let (input, header) = parse_header(input, swf_version)?;
  let tags = parse_tag_block_string(input, swf_version);

  Ok((&[][..], ast::Movie { header, tags }))
}

/// Parses the movie header from a completely loaded input.
fn parse_header(input: &[u8], swf_version: u8) -> NomResult<&[u8], ast::Header> {
  match crate::streaming::movie::parse_header(input, swf_version) {
    Ok(ok) => Ok(ok),
    Err(nom::Err::Incomplete(_)) => Err(nom::Err::Error(nom::error::Error::new(input, nom::error::ErrorKind::Complete))),
    Err(e) => Err(e),
  }
}

/// Parses the string of tags from a completely loaded input.
pub(crate) fn parse_tag_block_string(mut input: &[u8], swf_version: u8) -> Vec<ast::Tag> {
  let mut tags: Vec<ast::Tag> = Vec::new();
  loop {
    input = match parse_tag(input, swf_version) {
      (input, Some(tag)) => {
        tags.push(tag);
        input
      }
      (_, None) => return tags,
    }
  }
}
