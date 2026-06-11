require "sinatra"
require "nokogiri"

get "/" do
  doc = Nokogiri::HTML("<h1>Hello from Feluda</h1>")
  doc.at_css("h1").text
end
